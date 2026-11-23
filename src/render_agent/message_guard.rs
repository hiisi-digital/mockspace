//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The message guard a project chains in front of the message gate.
//!
//! The gate submits what a tool call authors to this repository's lints. A
//! project that keeps its own policy for published text, a guard written once
//! for every repository it holds, names that guard as `agent_message_guard` in
//! `<mock>/agent/config.toml`, and the gate hands it every tool call first.
//! A session opened inside one repository loads that repository's hooks alone,
//! so without the chain the project's guard never sees a call made there.
//!
//! The guard is a pre-tool-use hook of its own: the call's JSON on stdin, a
//! refusal as a `permissionDecision` of `deny` on stdout or as exit 2 with the
//! reason on stderr. Anything else it exits with is a guard that could not
//! decide, and the gate refuses rather than read that as a pass. Its path is
//! read against the repository's root; a guard that is not there is refused
//! too, since a chain that is configured and silently missing is the gap this
//! closes.

/// The shell that runs the guard, empty where none is configured.
pub(crate) fn message_guard_chain(guard: Option<&str>) -> String {
    let Some(guard) = guard.map(str::trim).filter(|g| !g.is_empty()) else {
        return String::new();
    };
    let quoted = format!("'{}'", guard.replace('\'', r"'\''"));
    format!(
        r##"# The project's own message guard, chained in front of the gate.
_GUARD={quoted}
case "$_GUARD" in /*) ;; *) _GUARD="$__HOOK_REPO_ROOT/$_GUARD" ;; esac
[ -x "$_GUARD" ] \
    || deny "the message guard $_GUARD this repository names as agent_message_guard is not there or not executable; it refuses rather than run without it."
# The guard reads the Claude shape whichever platform ran this: Copilot's
# `toolName` and `toolArgs` (an object, or a JSON string of one) become
# `tool_name` and `tool_input`, the rest of the payload kept.
_GIN=$(mktemp 2>/dev/null) \
    || deny "the message gate cannot run its guard: no scratch file for the call. It refuses rather than pass what the guard did not read."
_GERR=$(mktemp 2>/dev/null) \
    || {{ rm -f "$_GIN"; deny "the message gate cannot run its guard: no scratch file for its errors. It refuses rather than pass what the guard did not read."; }}
printf '%s' "$__INPUT" \
    | jq -c '. + {{tool_name: (.tool_name // .toolName // ""), tool_input: (.tool_input // (.toolArgs | if type == "string" then (fromjson? // {{}}) else (. // {{}}) end))}}' \
    > "$_GIN" 2>/dev/null \
    || {{ rm -f "$_GIN" "$_GERR"; deny "the message gate cannot hand its guard this call: the payload does not read as JSON. It refuses rather than pass what the guard did not read."; }}
# From a file, not a pipe: a guard that decides without reading all of a
# large payload would otherwise kill the writer and read as a failure.
_GOUT=$(cd "$__HOOK_REPO_ROOT" && "$_GUARD" < "$_GIN" 2>"$_GERR")
_GRC=$?
_GREASON=$(cat "$_GERR" 2>/dev/null || true)
rm -f "$_GIN" "$_GERR"
if [ "$_GRC" -eq 2 ]; then
    deny "${{_GREASON:-the message guard refused this call}}"
fi
[ "$_GRC" -eq 0 ] \
    || deny "the message guard $_GUARD exited $_GRC and could not decide; it refuses rather than pass what was not read. $_GREASON"
# A deny is carried through, and so is an ask: this gate has no way to ask,
# and a question the guard put is not an answer to pass on.
if printf '%s' "$_GOUT" | grep -qE '"permissionDecision"[[:space:]]*:[[:space:]]*"(deny|ask)"'; then
    deny "$(printf '%s' "$_GOUT" | jq -r '.hookSpecificOutput.permissionDecisionReason // .permissionDecisionReason // "the message guard refused this call"' 2>/dev/null)"
fi
"##

    )
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::process::{Command, Stdio};

    use super::super::*;

    fn gate(root: &std::path::Path, guard: Option<&str>) -> String {
        gate_with(root, guard, crate::render_agent::CLAUDE_HOOK_HELPERS)
    }

    fn gate_with(root: &std::path::Path, guard: Option<&str>, helpers: &str) -> String {
        let mut c = Config::from_dir(std::path::Path::new("/nonexistent-mock-dir"));
        c.project_name = "fodder".to_string();
        c.repo_root = root.to_path_buf();
        c.mock_dir = root.join("mock");
        c.agent.message_guard = guard.map(str::to_string);
        builtin_check_message(&c)
            .replace("{{HOOK_HELPERS}}", helpers)
            .replace("{{REPO_ROOT}}", &root.display().to_string())
    }

    fn run(root: &std::path::Path, script: &str, command: &str) -> String {
        let payload = serde_json::json!({
            "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
            "tool_input": { "command": command }
        })
        .to_string();
        run_payload(root, script, &payload)
    }

    fn run_payload(root: &std::path::Path, script: &str, payload: &str) -> String {
        let f = root.join("gate.sh");
        std::fs::write(&f, script).unwrap();
        let mut ch = Command::new("bash")
            .arg(&f)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        {
            use std::io::Write;
            ch.stdin
                .take()
                .unwrap()
                .write_all(payload.as_bytes())
                .unwrap();
        }
        String::from_utf8_lossy(&ch.wait_with_output().unwrap().stdout).to_string()
    }

    fn guard(root: &std::path::Path, body: &str) {
        let g = root.join("guard.sh");
        std::fs::write(&g, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&g, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn root() -> tempfile::TempDir {
        mockspace_manifest::scratch::tempdir().unwrap()
    }

    #[test]
    fn no_guard_configured_writes_no_chain() {
        assert_eq!(message_guard_chain(None), "");
        assert_eq!(message_guard_chain(Some("  ")), "");
        assert!(!gate(std::path::Path::new("/r"), None).contains("_GUARD"));
    }

    #[test]
    fn a_guard_that_denies_on_stdout_refuses_a_call_the_gate_would_pass() {
        // `ls` authors nothing, so the gate alone allows it: the refusal is the
        // guard's, carried through with its reason.
        let d = root();
        guard(
            d.path(),
            r#"cat >/dev/null; printf '%s\n' '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"no pull request body"}}'"#,
        );
        let out = run(d.path(), &gate(d.path(), Some("guard.sh")), "ls");
        assert!(out.contains("\"permissionDecision\":\"deny\""), "{out}");
        assert!(out.contains("no pull request body"), "{out}");
        // control: the same call with no guard is allowed
        let out = run(d.path(), &gate(d.path(), None), "ls");
        assert!(!out.contains("deny"), "{out}");
    }

    #[test]
    fn a_guard_that_exits_two_refuses_with_its_stderr() {
        let d = root();
        guard(
            d.path(),
            "cat >/dev/null; echo 'an advert in plain words' >&2; exit 2",
        );
        let out = run(d.path(), &gate(d.path(), Some("guard.sh")), "ls");
        assert!(
            out.contains("deny") && out.contains("an advert in plain words"),
            "{out}"
        );
    }

    #[test]
    fn a_guard_that_reads_the_call_sees_it_whole() {
        let d = root();
        guard(
            d.path(),
            r#"grep -q 'gh pr create' && { echo refused >&2; exit 2; }; exit 0"#,
        );
        let g = gate(d.path(), Some("guard.sh"));
        assert!(run(d.path(), &g, "gh pr create --title t --body x").contains("refused"));
        assert!(!run(d.path(), &g, "ls").contains("deny"));
    }

    #[test]
    fn a_guard_that_fails_or_is_missing_refuses_rather_than_passes() {
        let d = root();
        guard(d.path(), "cat >/dev/null; exit 1");
        let out = run(d.path(), &gate(d.path(), Some("guard.sh")), "ls");
        assert!(
            out.contains("deny") && out.contains("could not decide"),
            "{out}"
        );
        let out = run(d.path(), &gate(d.path(), Some("missing.sh")), "ls");
        assert!(out.contains("deny") && out.contains("not there"), "{out}");
    }

    #[test]
    fn the_guard_is_read_from_the_agent_config_and_reaches_the_gate() {
        let fx = super::super::tests::skill_fixture_config(
            "agent_message_guard = \"../scripts/guard.sh\"\n",
        );
        let cfg = crate::config::Config::from_dir(&fx.path().join("mock"));
        assert_eq!(
            cfg.agent.message_guard.as_deref(),
            Some("../scripts/guard.sh")
        );
        assert!(builtin_check_message(&cfg).contains("_GUARD='../scripts/guard.sh'"));
        let bare = super::super::tests::skill_fixture_config("");
        let cfg = crate::config::Config::from_dir(&bare.path().join("mock"));
        assert_eq!(cfg.agent.message_guard, None);
    }

    /// A guard that reads the Claude shape, as the workspace's does.
    const READS_CLAUDE_SHAPE: &str = r#"in=$(cat)
tool=$(printf '%s' "$in" | jq -r '.tool_name // ""')
cmd=$(printf '%s' "$in" | jq -r '.tool_input.command // ""')
case "$tool:$cmd" in Bash:*"gh pr create"*) echo "no pull request body" >&2; exit 2 ;; esac
exit 0"#;

    #[test]
    fn a_copilot_call_reaches_the_guard_in_the_claude_shape() {
        // Copilot hands `toolName` and `toolArgs`, as an object or as a JSON
        // string of one; a guard reading `tool_name` would see nothing and pass.
        let d = root();
        guard(d.path(), READS_CLAUDE_SHAPE);
        let g = gate_with(
            d.path(),
            Some("guard.sh"),
            crate::render_agent::COPILOT_HOOK_HELPERS,
        );
        let args = serde_json::json!({ "command": "gh pr create --title t --body x" });
        for tool_args in [args.clone(), serde_json::Value::String(args.to_string())] {
            let payload = serde_json::json!({ "toolName": "Bash", "toolArgs": tool_args }).to_string();
            let out = run_payload(d.path(), &g, &payload);
            assert!(out.contains("deny") && out.contains("no pull request body"), "{out}");
        }
        // control: a Copilot call the guard has no quarrel with passes
        let payload = serde_json::json!({ "toolName": "Bash", "toolArgs": { "command": "ls" } }).to_string();
        assert!(!run_payload(d.path(), &g, &payload).contains("deny"));
    }

    #[test]
    fn a_guard_that_answers_without_reading_a_large_call_is_not_a_failure() {
        // Fed by a pipe, the writer dies on a full buffer when the guard never
        // reads, and the call was refused as one the guard could not decide.
        let d = root();
        guard(d.path(), "exit 0");
        let big = "x".repeat(256 * 1024);
        let out = run(d.path(), &gate(d.path(), Some("guard.sh")), &format!("echo {big}"));
        assert!(!out.contains("could not decide"), "{}", &out[.. out.len().min(300)]);
    }

    #[test]
    fn a_guard_that_asks_is_refused_with_its_question() {
        let d = root();
        guard(
            d.path(),
            r#"cat >/dev/null; printf '%s\n' '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"ask","permissionDecisionReason":"is this body yours"}}'"#,
        );
        let out = run(d.path(), &gate(d.path(), Some("guard.sh")), "ls");
        assert!(out.contains("deny") && out.contains("is this body yours"), "{out}");
    }

    #[test]
    fn a_path_with_a_quote_stays_one_word() {
        let d = root();
        let dir = d.path().join("it's");
        std::fs::create_dir(&dir).unwrap();
        guard(&dir, "cat >/dev/null; echo quoted >&2; exit 2");
        let out = run(d.path(), &gate(d.path(), Some("it's/guard.sh")), "ls");
        assert!(out.contains("quoted"), "{out}");
    }
}
