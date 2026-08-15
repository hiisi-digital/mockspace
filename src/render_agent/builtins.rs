#![allow(unused_imports)]
use super::*;

/// Generate the check-message.sh hook: route every authored message a tool is
/// about to produce through the configured message lints.
///
/// # The hole this closes
///
/// The predecessor matched `git commit` inside `tool_input.command` and nothing
/// else. Three consequences, all of which let real violations through for months:
///
/// 1. It fired for `Bash` only. When first-class `git-commit`-style MCP tools
///    arrived, they were not `Bash`, so the hook never ran and every commit made
///    through one was unchecked.
/// 2. It never looked at `gh pr create`, so pull-request bodies were never
///    checked by anything. A forge body does not pass through git either, so no
///    git hook covered them; there was no layer at all.
/// 3. It extracted trailers with `grep -oiE 'co-authored-by:[^"]*'`, whose
///    `[^"]` stops at the first double quote, so a heredoc or single-quoted body
///    was mis-read.
///
/// So this hook matches **every** tool, decides relevance from the tool name and
/// the whole serialised input rather than from one field, and reads any file
/// passed by `-F` or `--body-file` so a message handed over indirectly is still
/// inspected. Policy itself lives in the lints; this only decides what to submit
/// and under which domain.
pub(crate) fn builtin_check_message(cfg: &Config) -> String {
    let mock_rel = cfg
        .mock_dir
        .strip_prefix(&cfg.repo_root)
        .unwrap_or(&cfg.mock_dir)
        .to_string_lossy()
        .to_string();
    let _ = mock_rel;
    format!(
        r##"#!/usr/bin/env bash
# @matchers: *
# Built-in mockspace hook: submit authored messages to the configured lints.
#
# Matches EVERY tool on purpose. A git-commit or pull-request MCP tool is not
# Bash, and the Bash-only predecessor is exactly why bylines and adverts went
# unnoticed. Relevance is decided below, from the tool name and the whole input.
set -uo pipefail
__INPUT=$(cat)
{{{{HOOK_HELPERS}}}}

# Fail closed without jq. Every field below comes from it, and with empty
# fields DOMAIN stays empty and the gate allows everything: a gate reporting
# success while enforcing nothing, the exact failure this hook replaces.
command -v jq >/dev/null 2>&1 \
    || deny "the message gate cannot run: jq is not on PATH. It refuses rather than passing what it cannot read; install jq and retry."

TOOL_NAME=$(echo "$__INPUT" | jq -r '.tool_name // ""' 2>/dev/null || echo "")
COMMAND=$(_extract "command")
FILE_PATH=""

# The ENTIRE tool input, serialised. Scanning this rather than one field is what
# makes a heredoc, a single-quoted body, a `--body` argument and an MCP tool's
# structured fields all visible to the same check.
INPUT_ALL=$(echo "$__INPUT" | jq -r '.tool_input | tostring' 2>/dev/null || echo "")

# Only this repo's business.
#
# The shared `_scope_or_allow` cannot be used here: it allows when both
# FILE_PATH and COMMAND are empty, which is every MCP tool, and those are the
# tools this hook exists to cover. Scope on the process cwd instead, which is
# where an MCP tool operates, falling back to the command's mention of the repo.
_in_scope=1
case "$(pwd)" in
    "$__HOOK_REPO_ROOT"|"$__HOOK_REPO_ROOT"/*) _in_scope=0 ;;
esac
if [ "$_in_scope" != "0" ] && [ -n "$COMMAND" ]; then
    if echo "$COMMAND" | grep -qF "$__HOOK_REPO_ROOT"; then _in_scope=0; fi
fi
if [ "$_in_scope" != "0" ] && [ -n "$INPUT_ALL" ]; then
    if printf '%s' "$INPUT_ALL" | grep -qF "$__HOOK_REPO_ROOT"; then _in_scope=0; fi
fi
[ "$_in_scope" = "0" ] || allow

# ---------------------------------------------------------------------------
# Which surface is this, if any?
# ---------------------------------------------------------------------------
DOMAIN=""
_lower_tool=$(printf '%s' "$TOOL_NAME" | tr '[:upper:]' '[:lower:]')

# Forge tools, by name shape. `pr-`, `-pr` and `-pr-` catch the MCP naming
# conventions in the wild without needing a list of every server.
case "$_lower_tool" in
    # `pr` as a whole word in the name, however the server spells its
    # separators: create_pr, pr-create, createPullRequest, merge_request.
    *pr-*|*-pr|*_pr|*pr_*|*-pr-*|*_pr_*|*pull?request*|*pullrequest*|*merge?request*)
        DOMAIN="pull-request-body" ;;
esac

# Git tools that author a message.
if [ -z "$DOMAIN" ]; then
    case "$_lower_tool" in
        *commit*|*git?push*|*git_push*) DOMAIN="commit-message" ;;
    esac
fi

# Shell commands. Checked after tool names so an MCP tool that also carries a
# command string is classified by what it is rather than by what it wraps.
if [ -z "$DOMAIN" ] && [ -n "$COMMAND" ]; then
    if echo "$COMMAND" | grep -qE '\bgh[[:space:]]+(pr|issue|release|gist)\b' \
       || echo "$COMMAND" | grep -qE '\bglab[[:space:]]+(mr|issue|release)\b'; then
        DOMAIN="pull-request-body"
    elif echo "$COMMAND" | grep -qE '\bgit[[:space:]]+(-C[[:space:]]+[^[:space:]]+[[:space:]]+)?(commit|tag|notes|merge|revert|cherry-pick|am)\b'; then
        DOMAIN="commit-message"
    fi
fi

# Nothing that authors a durable message: not this hook's concern.
[ -z "$DOMAIN" ] && allow

# ---------------------------------------------------------------------------
# Gather the text to inspect
# ---------------------------------------------------------------------------
SUBMIT="$INPUT_ALL"

# A message handed over by file reference is still a message. `-F`, `--file`,
# `--body-file` and their `=` forms all name one, and the predecessor read none
# of them, so writing the body to a file was a complete bypass.
_read_referenced_files() {{
    local text="$1" f
    for f in $(printf '%s\n' "$text" \
        | grep -oE -- '(--body-file|--file|-F)[= ]+[^ "'"'"']+' \
        | sed -E 's/^(--body-file|--file|-F)[= ]+//'); do
        case "$f" in
            -*) continue ;;                 # another flag, not a path
        esac
        if [ -f "$f" ]; then
            printf '\n%s\n' "$(cat "$f" 2>/dev/null || true)"
        fi
    done
}}
SUBMIT="$SUBMIT$(_read_referenced_files "$COMMAND")"
SUBMIT="$SUBMIT$(_read_referenced_files "$INPUT_ALL")"

# Nothing substantive to check.
if [ -z "$(printf '%s' "$SUBMIT" | tr -d '[:space:]')" ]; then allow; fi

# ---------------------------------------------------------------------------
# Verdict, with a short-lived cache
# ---------------------------------------------------------------------------
# The same message is commonly seen twice within seconds: once here, before the
# command runs, and again by the commit-msg git hook once it does. The cache
# makes the second lookup free without ever making a stale verdict reusable.
_CACHE_DIR="${{TMPDIR:-/tmp}}/mockspace-message-cache"
_CACHE_TTL=30
mkdir -p "$_CACHE_DIR" 2>/dev/null || true
# Keyed on the repo, the policy content, the domain and the text. Without the
# first two, two repos with different [attribution] policy and one identical
# message shared a verdict for the TTL, so a byline permitted in one passed in
# one that forbids it.
_KEY=$({{ printf '%s\0%s\0%s\0' "$__HOOK_REPO_ROOT" "$DOMAIN" "$SUBMIT"; cat "$__HOOK_REPO_ROOT/{mock_rel}/agent/config.toml" 2>/dev/null || true; }} | cksum | tr -d ' ')
_CACHE_FILE="$_CACHE_DIR/$_KEY"

_cached_verdict() {{
    [ -f "$_CACHE_FILE" ] || return 1
    local now mtime age v
    now=$(date +%s)
    # stat is not portable between GNU and BSD; try both spellings.
    mtime=$(stat -f %m "$_CACHE_FILE" 2>/dev/null || stat -c %Y "$_CACHE_FILE" 2>/dev/null || echo 0)
    age=$((now - mtime))
    [ "$age" -ge 0 ] && [ "$age" -le "$_CACHE_TTL" ] || return 1
    v=$(cat "$_CACHE_FILE" 2>/dev/null || echo "")
    # Only the two verdicts we write are valid; anything else is a corrupt or
    # partially written entry and is treated as a miss.
    case "$v" in
        pass|fail) printf '%s' "$v"; return 0 ;;
        *) return 1 ;;
    esac
}}

if _V=$(_cached_verdict); then
    if [ "$_V" = "pass" ]; then allow; fi
    deny "message policy rejected this ${{DOMAIN}} (cached within ${{_CACHE_TTL}}s). Run the command again after fixing it to re-check."
fi

launcher=""
if command -v mock >/dev/null 2>&1; then launcher="mock"
elif command -v cargo-mock >/dev/null 2>&1; then launcher="cargo-mock"
fi
if [ -z "$launcher" ]; then
    deny "the message gate cannot run: no mockspace launcher on PATH. Policy is configured in {mock_rel}/agent/config.toml and enforced by the engine, so guessing a weaker rule here would contradict it. Install with: cargo install cargo-mock"
fi

_OUT=$(printf '%s\n' "$SUBMIT" \
    | "$launcher" check-message --domain "$DOMAIN" --gate commit \
        --command "$COMMAND" --tool "$TOOL_NAME" 2>&1)
_RC=$?

if [ "$_RC" -eq 0 ]; then
    printf 'pass' > "$_CACHE_FILE" 2>/dev/null || true
    allow
fi

printf 'fail' > "$_CACHE_FILE" 2>/dev/null || true
# Collapse to one line: the deny reason is JSON-embedded.
_REASON=$(printf '%s' "$_OUT" | tr '\n' ' ' | sed 's/"/\\"/g')
deny "message policy rejected this ${{DOMAIN}}. $_REASON"
"##
    )
}

/// Generate the mockspace-write-guard.sh hook content.
pub(crate) fn builtin_write_guard(cfg: &Config) -> String {
    let mock_rel = cfg
        .mock_dir
        .strip_prefix(&cfg.repo_root)
        .unwrap_or(&cfg.mock_dir)
        .to_string_lossy()
        .to_string();
    format!(
        r##"#!/usr/bin/env bash
# Built-in mockspace hook: phase gate enforcement
# Mirrors the lint rules: changelist_doc_gate, changelist_required,
# changelist_lock, changelist_immutability.
set -uo pipefail
__INPUT=$(cat)
MOCK_ROOT="{mock_rel}"
{{{{HOOK_HELPERS}}}}
FILE_PATH=$(_extract file_path)
COMMAND=$(_extract command)
_scope_or_allow
# --- Determine target path ---
TARGET=""
if [[ -n "$FILE_PATH" ]]; then
    TARGET="$FILE_PATH"
elif [[ -n "$COMMAND" ]]; then
    # Mutation-only detection: match commands that write to files. The
    # patterns require either a leading space/start-of-string to avoid
    # matching substrings in unrelated words, or a trailing space to
    # avoid matching word-ending tokens. Shell FD redirects (`2>&1`,
    # `&> file`) are intentionally NOT classified as file writes: they
    # redirect descriptors, not mutate files. Actual file redirection
    # (`> path`, `>> path`) goes through Write/Edit tools in the agent
    # flow, not Bash; we accept the tradeoff.
    WRITE_MARKERS='(^|[[:space:]])(tee|mv|cp|rm|dd|install|sed[[:space:]]+-i|perl[[:space:]]+-i)([[:space:]]|$)'
    if ! echo "$COMMAND" | grep -qE "$WRITE_MARKERS"; then
        allow
    fi
    if echo "$COMMAND" | grep -q "$MOCK_ROOT"; then
        TARGET=$(echo "$COMMAND" | grep -oE "[^ ]*${{MOCK_ROOT}}[^ ]*" | head -1) || true
    fi
fi
# Not targeting the mock subdir? Allow (scope-to-repo already passed).
if [[ -z "$TARGET" ]] || ! echo "$TARGET" | grep -q "$MOCK_ROOT"; then
    allow
fi
REL_PATH=$(echo "$TARGET" | sed "s|.*${{MOCK_ROOT}}/||")
# --- Always allowed: agent templates ---
if echo "$REL_PATH" | grep -qE '^agent/'; then
    allow
fi
# --- Always allowed: root-level mockspace templates ---
if echo "$REL_PATH" | grep -qE '^[^/]+\.md\.tmpl$'; then
    allow
fi
REPO_ROOT=$(git rev-parse --show-toplevel 2>/dev/null || echo "")
if [[ -z "$REPO_ROOT" ]]; then
    allow
fi
# --- Detect phase ---
# All git ls-files queries run with `-C "$REPO_ROOT"` so phase detection is
# cwd-independent. Without it, invocations from a subdir (e.g. `mock/`) would
# resolve `mock/design_rounds/` relative to cwd, return nothing, and fall back
# to TOPIC, blocking legitimate IMPL-phase edits.
DOC_CL_ACTIVE=$(git -C "$REPO_ROOT" ls-files "${{MOCK_ROOT}}/design_rounds/" 2>/dev/null \
    | grep -E "^${{MOCK_ROOT}}/design_rounds/[^/]+_changelist\.doc\.md$" \
    | head -1) || true
DOC_CL_LOCKED=$(git -C "$REPO_ROOT" ls-files "${{MOCK_ROOT}}/design_rounds/" 2>/dev/null \
    | grep -E "^${{MOCK_ROOT}}/design_rounds/[^/]+_changelist\.doc\.lock\.md$" \
    | head -1) || true
SRC_CL_ACTIVE=$(git -C "$REPO_ROOT" ls-files "${{MOCK_ROOT}}/design_rounds/" 2>/dev/null \
    | grep -E "^${{MOCK_ROOT}}/design_rounds/[^/]+_changelist\.src\.md$" \
    | head -1) || true
SRC_CL_LOCKED=$(git -C "$REPO_ROOT" ls-files "${{MOCK_ROOT}}/design_rounds/" 2>/dev/null \
    | grep -E "^${{MOCK_ROOT}}/design_rounds/[^/]+_changelist\.src\.lock\.md$" \
    | head -1) || true
if [[ -n "$SRC_CL_LOCKED" ]]; then
    PHASE="CLOSED"
elif [[ -n "$SRC_CL_ACTIVE" ]]; then
    PHASE="IMPL"
elif [[ -n "$DOC_CL_LOCKED" ]]; then
    PHASE="DRAFT"
elif [[ -n "$DOC_CL_ACTIVE" ]]; then
    PHASE="DOC"
else
    PHASE="TOPIC"
fi
DIRTY_DOCS=$(git -C "$REPO_ROOT" diff --name-only 2>/dev/null \
    | grep -E "^${{MOCK_ROOT}}/crates/.*\.(md\.tmpl|md)$" | head -1) || true
# --- Design round files ---
if echo "$REL_PATH" | grep -qE '^design_rounds/'; then
    FULL_GIT_PATH="${{MOCK_ROOT}}/${{REL_PATH}}"
    BASENAME=$(basename "$REL_PATH")
    if [[ "$BASENAME" == "README.md" ]]; then
        allow
    fi
    if echo "$REL_PATH" | grep -qE '^design_rounds/[^/]+/'; then
        SUBDIR_NAME=$(echo "$REL_PATH" | sed -n 's|^design_rounds/\([^/]*\)/.*|\1|p')
        FLAT_PATH=$(echo "$REL_PATH" | sed 's|^design_rounds/[^/]*/|design_rounds/|')
        deny "BLOCKED: '${{REL_PATH}}' targets a round-archive subdirectory.\\n\\nSubdirectories under design_rounds/ are ARCHIVED closed rounds; they are frozen forever.\\nActive rounds always live FLAT at design_rounds/ root (see design-round skill, File layout section).\\n\\nIf you meant to edit an active round file, the correct path is probably:\\n  ${{FLAT_PATH}}\\n\\nIf that active file does not yet exist, create it at the flat path above.\\nNever mkdir under design_rounds/ for active work; cargo mock close is the only process that creates subdirectories.\\n\\nCurrent phase: ${{PHASE}}"
    fi
    IS_CHANGELIST=false
    IS_DOC_CHANGELIST=false
    IS_SRC_CHANGELIST=false
    IS_LOCKED=false
    IS_DEPRECATED=false
    if echo "$BASENAME" | grep -qE '_changelist\.(doc|src)\.(lock|deprecated)\.md$'; then
        IS_CHANGELIST=true
        echo "$BASENAME" | grep -q '\.doc\.' && IS_DOC_CHANGELIST=true
        echo "$BASENAME" | grep -q '\.src\.' && IS_SRC_CHANGELIST=true
        echo "$BASENAME" | grep -q '\.lock\.' && IS_LOCKED=true
        echo "$BASENAME" | grep -q '\.deprecated\.' && IS_DEPRECATED=true
    elif echo "$BASENAME" | grep -qE '_changelist\.(doc|src)\.md$'; then
        IS_CHANGELIST=true
        echo "$BASENAME" | grep -q '\.doc\.' && IS_DOC_CHANGELIST=true
        echo "$BASENAME" | grep -q '\.src\.' && IS_SRC_CHANGELIST=true
    fi
    if git -C "$REPO_ROOT" ls-files --error-unmatch "$FULL_GIT_PATH" >/dev/null 2>&1; then
        if $IS_CHANGELIST && $IS_LOCKED; then
            deny "BLOCKED: locked changelist '${{BASENAME}}' is FROZEN.\\n\\nCurrent phase: ${{PHASE}}\\nLint: changelist-immutability (HARD_ERROR)"
        fi
        if $IS_CHANGELIST && $IS_DEPRECATED; then
            deny "BLOCKED: deprecated changelist '${{BASENAME}}' is FROZEN.\\n\\nCurrent phase: ${{PHASE}}\\nLint: changelist-immutability (HARD_ERROR)"
        fi
        if $IS_DOC_CHANGELIST && ! $IS_LOCKED && ! $IS_DEPRECATED; then
            if [[ "$PHASE" == "DOC" ]]; then allow; fi
            deny "BLOCKED: cannot edit doc changelist '${{BASENAME}}' -- not in DOC phase.\\n\\nPhase: ${{PHASE}}.\\nLint: changelist-immutability (HARD_ERROR)"
        fi
        if $IS_SRC_CHANGELIST && ! $IS_LOCKED && ! $IS_DEPRECATED; then
            if [[ "$PHASE" == "IMPL" ]]; then allow; fi
            deny "BLOCKED: cannot edit source changelist '${{BASENAME}}' -- not in IMPL phase.\\n\\nPhase: ${{PHASE}}.\\nLint: changelist-immutability (HARD_ERROR)"
        fi
        if ! $IS_CHANGELIST; then
            deny "BLOCKED: topic '${{BASENAME}}' is committed and FROZEN.\\n\\nCurrent phase: ${{PHASE}}"
        fi
    fi
    if ! $IS_CHANGELIST; then
        if [[ "$PHASE" != "TOPIC" ]]; then
            deny "BLOCKED: cannot create topic '${{BASENAME}}' -- not in TOPIC phase.\\n\\nPhase: ${{PHASE}}."
        fi
        allow
    fi
    if $IS_DOC_CHANGELIST && ! $IS_LOCKED && ! $IS_DEPRECATED; then
        if [[ "$PHASE" != "TOPIC" ]]; then
            deny "BLOCKED: cannot create doc changelist '${{BASENAME}}' -- one already exists or phase is wrong.\\n\\nPhase: ${{PHASE}}."
        fi
        allow
    fi
    if $IS_SRC_CHANGELIST && ! $IS_LOCKED && ! $IS_DEPRECATED; then
        if [[ "$PHASE" != "DRAFT" ]]; then
            deny "BLOCKED: cannot create source changelist '${{BASENAME}}' -- not in DRAFT phase.\\n\\nPhase: ${{PHASE}}."
        fi
        allow
    fi
    if $IS_LOCKED || $IS_DEPRECATED; then allow; fi
    allow
fi
# --- Crate files are phase-gated ---
if echo "$REL_PATH" | grep -qE '^crates/'; then
    if echo "$REL_PATH" | grep -qE '(^|/)SHAME\.md\.tmpl$'; then allow; fi
    if echo "$REL_PATH" | grep -qE '\.(md\.tmpl|md)$'; then
        if [[ "$PHASE" != "DOC" ]]; then
            deny "BLOCKED: cannot edit '${{REL_PATH}}' -- not in DOC phase.\\n\\nPhase: ${{PHASE}}.\\nLint: changelist-doc-gate (HARD_ERROR)"
        fi
        allow
    fi
    if echo "$REL_PATH" | grep -qE '\.rs$'; then
        if [[ "$PHASE" != "IMPL" ]]; then
            deny "BLOCKED: cannot edit '${{REL_PATH}}' -- not in IMPL phase.\\n\\nPhase: ${{PHASE}}.\\nLint: changelist-required (HARD_ERROR)"
        fi
        allow
    fi
    if [[ "$PHASE" == "TOPIC" ]] || [[ "$PHASE" == "CLOSED" ]]; then
        deny "BLOCKED: cannot edit '${{REL_PATH}}' -- no changelist exists or round is complete.\\n\\nPhase: ${{PHASE}}."
    fi
    allow
fi
# --- Root Cargo.toml ---
if echo "$REL_PATH" | grep -qE '^Cargo\.toml$'; then
    if [[ "$PHASE" == "TOPIC" ]] || [[ "$PHASE" == "CLOSED" ]]; then
        deny "BLOCKED: cannot edit '${{REL_PATH}}' -- no changelist exists or round is complete.\\n\\nPhase: ${{PHASE}}."
    fi
    allow
fi
allow
"##
    )
}

/// Generate the mockspace-reminder.sh hook content.
pub(crate) fn builtin_reminder(cfg: &Config) -> String {
    let mock_rel = cfg
        .mock_dir
        .strip_prefix(&cfg.repo_root)
        .unwrap_or(&cfg.mock_dir)
        .to_string_lossy()
        .to_string();
    format!(
        r##"#!/usr/bin/env bash
# Built-in mockspace hook: print mockspace rules reminder before tool use
# Non-blocking. Only fires when the tool targets mockspace paths.
set -uo pipefail
__INPUT=$(cat)
{{{{HOOK_HELPERS}}}}
FILE_PATH=$(_extract file_path)
COMMAND=$(_extract command)
_scope_or_allow
IS_MOCKSPACE=false
if [[ -n "$FILE_PATH" ]] && echo "$FILE_PATH" | grep -q "{mock_rel}"; then
    IS_MOCKSPACE=true
fi
if [[ -n "$COMMAND" ]] && echo "$COMMAND" | grep -q "{mock_rel}"; then
    IS_MOCKSPACE=true
fi
if [[ "$IS_MOCKSPACE" != "true" ]]; then
    allow
fi
context "MOCKSPACE REMINDER: You are operating on mockspace files. Follow the design round workflow: TOPIC -> DOC -> DRAFT -> IMPL -> CLOSED. Check phase before editing. Use 'cargo mock' commands for phase transitions."
allow
"##
    )
}

/// Generate the no-yagni-guard.sh hook content.
/// The durable agent-side gate. Fires on Bash; for a git commit or push,
/// it refuses to let the agent proceed unless the mockspace validation
/// machinery is intact (the generated validator exists and core.hooksPath
/// is wired to it). If broken by a `target/` clean it self-heals via a
/// `cargo check`, and blocks hard if that fails. Native mockspace, so it
/// travels with `.claude/hooks/` and survives independently of git config.
/// The self-heal step runs the launcher rather than `cargo check`.
///
/// It used to run `cargo check` in the mock workspace, on the reasoning that
/// `build.rs` re-ran the bootstrap and re-activated the gate. That bootstrap
/// is gone and `bootstrap_from_buildscript` is now a tombstone that fails the
/// build with migration steps, so the step could not restore anything and
/// cost a full check on every broken-gate path. A bare `cargo mock` is what
/// writes the validator and points `core.hooksPath`, and it is what the deny
/// message already tells the user to run.
pub(crate) fn builtin_mockspace_gate() -> String {
    r##"#!/usr/bin/env bash
# Built-in mockspace hook: block agent git commit/push when the gate is broken
set -uo pipefail
__INPUT=$(cat)
{{HOOK_HELPERS}}
FILE_PATH=""
COMMAND=$(_extract "command")
_scope_or_allow

# Only gate git commit / git push; everything else passes through. Anchor
# the verb to the git subcommand position (allowing a leading `-C <dir>`)
# so read-only commands like `git log --grep=commit` or `git help commit`
# are not mistaken for a write.
if ! echo "$COMMAND" | grep -qE '\bgit[[:space:]]+(commit|push)\b' \
   && ! echo "$COMMAND" | grep -qE '\bgit[[:space:]]+-C[[:space:]]+[^[:space:]]+[[:space:]]+(commit|push)\b'; then
    allow
fi

root="$__HOOK_REPO_ROOT"
mockdir=$(git -C "$root" config --local mockspace.mockdir 2>/dev/null || true)
[ -z "$mockdir" ] && mockdir="mock"
validator="$root/$mockdir/target/hooks/pre-commit"

_gate_intact() {
    [ -x "$validator" ] || return 1
    local hp
    hp=$(git -C "$root" config --local core.hooksPath 2>/dev/null || true)
    case "$hp" in
        *mockspace*|*target/hooks*) return 0 ;;
        *) return 1 ;;
    esac
}

if _gate_intact; then allow; fi

# Gate broken: validator cleaned away, or core.hooksPath not wired.
#
# Try the narrow repair first. `activate` only ensures the durable hooks and
# points core.hooksPath; it writes nothing else in the tree, which matters
# because this runs while a commit is in flight. Only if that is not enough
# (the validator itself is missing, which activate refuses to paper over) do
# we fall back to a full run, which also regenerates docs and agent rules.
( cd "$root" && cargo mock activate ) >/dev/null 2>&1 || true
if _gate_intact; then allow; fi
( cd "$root" && cargo mock ) >/dev/null 2>&1 || true
if _gate_intact; then allow; fi

deny "mockspace gate is broken and self-heal failed: the git validator at $validator is missing or core.hooksPath is not wired to mockspace. Agent commits and pushes are blocked until it is restored. Run 'cargo mock' from $root, then retry."
"##
    .to_string()
}

pub(crate) fn builtin_no_yagni() -> String {
    r##"#!/usr/bin/env bash
# Built-in mockspace hook: flag YAGNI reasoning in commit messages
set -uo pipefail
__INPUT=$(cat)
{{HOOK_HELPERS}}
FILE_PATH=""
COMMAND=$(_extract "command")
_scope_or_allow
# Only check git commit commands
if ! echo "$COMMAND" | grep -q "git commit"; then allow; fi
# Check for YAGNI-like keywords in the commit message
YAGNI_PATTERNS="yagni|premature|over-engineer|overkill|good enough for now|not needed yet|too early|unnecessary complexity|keep it simple"
if echo "$COMMAND" | grep -qiE "$YAGNI_PATTERNS"; then
    context "WARNING: YAGNI-flavored reasoning detected in commit message. This project embraces the ideal when designing -- extensible, trait-based, registered. Shortcuts justified by 'you ain't gonna need it' are not welcome. Reconsider the commit message."
fi
allow
"##
    .to_string()
}

#[cfg(test)]
mod check_message_tests {
    use super::*;
    use std::io::Write;
    use std::process::{Command, Stdio};

    fn cfg_for(name: &str) -> Config {
        let mut c = Config::from_dir(std::path::Path::new("/nonexistent-mock-dir"));
        c.project_name = name.to_string();
        c
    }

    /// The hook script, with the shared helpers substituted as the renderer does.
    fn hook_script(repo_root: &std::path::Path) -> String {
        let mut cfg = cfg_for("fodder");
        cfg.repo_root = repo_root.to_path_buf();
        cfg.mock_dir = repo_root.join("mock");
        let raw = builtin_check_message(&cfg);
        raw.replace("{{HOOK_HELPERS}}", crate::render_agent::CLAUDE_HOOK_HELPERS)
            .replace("{{REPO_ROOT}}", &repo_root.display().to_string())
    }

    /// Run the hook with a given tool payload. Returns (exit code, stdout).
    fn run(repo_root: &std::path::Path, payload: &str, launcher: Option<&str>) -> (i32, String) {
        run_env(repo_root, payload, launcher, true)
    }

    fn run_env(
        repo_root: &std::path::Path,
        payload: &str,
        launcher: Option<&str>,
        with_jq: bool,
    ) -> (i32, String) {
        let dir = repo_root.join(format!("h{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("hook.sh");
        std::fs::write(&script, hook_script(repo_root)).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut p = std::fs::metadata(&script).unwrap().permissions();
            p.set_mode(0o755);
            std::fs::set_permissions(&script, p).unwrap();
        }
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        if let Some(body) = launcher {
            let m = bin.join("mock");
            std::fs::write(&m, body).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut p = std::fs::metadata(&m).unwrap().permissions();
                p.set_mode(0o755);
                std::fs::set_permissions(&m, p).unwrap();
            }
        }
        // A cache dir per invocation, so one test's verdict never leaks into
        // another's. The cache is a real behaviour and must not couple tests.
        let cache = dir.join("cache");
        std::fs::create_dir_all(&cache).unwrap();
        // A PATH holding the stub dir plus only what the hook genuinely needs
        // (jq, coreutils). The inherited PATH must NOT be used: it contains the
        // real `mock`, which would make the no-launcher case unreachable and let
        // a test invoke the actual engine.
        let jq_dir = Command::new("sh")
            .arg("-c")
            .arg("command -v jq")
            .output()
            .ok()
            .and_then(|o| {
                let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                std::path::Path::new(&s)
                    .parent()
                    .map(|p| p.display().to_string())
            })
            .unwrap_or_default();
        // Without jq the PATH holds only the stub dir plus a shim carrying the
        // one external the script touches before its jq guard, so the guard is
        // genuinely exercised even on systems that keep jq in /usr/bin.
        let path = if with_jq {
            format!("{}:{jq_dir}:/usr/bin:/bin", bin.display())
        } else {
            let shim = dir.join("shim");
            std::fs::create_dir_all(&shim).unwrap();
            #[cfg(unix)]
            for tool in ["bash", "sh", "cat", "printf"] {
                let real = Command::new("sh")
                    .arg("-c")
                    .arg(format!("command -v {tool}"))
                    .output()
                    .ok()
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                    .unwrap_or_default();
                if !real.is_empty() {
                    let _ = std::os::unix::fs::symlink(&real, shim.join(tool));
                }
            }
            format!("{}:{}", bin.display(), shim.display())
        };
        let mut child = Command::new(&script)
            .current_dir(repo_root)
            .env("PATH", path)
            .env("TMPDIR", &cache)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(payload.as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).to_string(),
        )
    }

    fn scratch() -> std::path::PathBuf {
        // A process-wide counter, not a clock. SystemTime here has microsecond
        // resolution at best (twenty back-to-back samples yield four distinct
        // values on this host), and `create_dir_all` succeeds silently on an
        // existing directory, so two tests starting in the same tick shared a
        // scratch directory and overwrote each other's fixtures.
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let d = std::env::temp_dir().join(format!(
            "ms_checkmsg_{}_{}",
            std::process::id(),
            SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    const PASS: &str = "#!/usr/bin/env bash\nexit 0\n";
    const FAIL: &str = "#!/usr/bin/env bash\necho 'x message-attribution: nope'\nexit 1\n";

    fn denied(out: &str) -> bool {
        out.contains("\"permissionDecision\":\"deny\"")
    }

    #[test]
    fn the_generated_hook_is_valid_bash() {
        // A syntax error here fails open: the hook errors, the tool proceeds, and
        // nothing is checked. That is worse than having no hook, because it looks
        // installed.
        let root = scratch();
        let script = hook_script(&root);
        let f = root.join("s.sh");
        std::fs::write(&f, &script).unwrap();
        let out = Command::new("bash").arg("-n").arg(&f).output().unwrap();
        let _ = std::fs::remove_dir_all(&root);
        assert!(
            out.status.success(),
            "bash -n rejected the hook:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn it_matches_every_tool_not_just_bash() {
        // The one-line cause of the whole bug: scoped to Bash, an MCP git tool
        // was never intercepted.
        let root = scratch();
        assert!(hook_script(&root).contains("# @matchers: *"));
        let _ = std::fs::remove_dir_all(&root);
    }

    // --- one case per row of the defence matrix ---

    #[test]
    fn a_bash_git_commit_is_submitted() {
        let root = scratch();
        let payload = format!(
            r#"{{"tool_name":"Bash","tool_input":{{"command":"cd {} && git commit -m 'feat: x'"}}}}"#,
            root.display()
        );
        let (_, out) = run(&root, &payload, Some(FAIL));
        let _ = std::fs::remove_dir_all(&root);
        assert!(denied(&out), "a bash git commit must be checked: {out}");
    }

    #[test]
    fn a_bash_gh_pr_create_is_submitted() {
        // Never covered by anything before: a forge body does not pass through
        // git, so no git hook could ever see one.
        let root = scratch();
        let payload = format!(
            r#"{{"tool_name":"Bash","tool_input":{{"command":"cd {} && gh pr create --body 'x'"}}}}"#,
            root.display()
        );
        let (_, out) = run(&root, &payload, Some(FAIL));
        let _ = std::fs::remove_dir_all(&root);
        assert!(denied(&out), "a gh pr create must be checked: {out}");
    }

    #[test]
    fn an_mcp_git_commit_tool_is_submitted() {
        // The regression that let months of bylines through.
        let root = scratch();
        let payload = format!(
            r#"{{"tool_name":"mcp__git__git_commit","tool_input":{{"message":"feat: x","repo":"{}"}}}}"#,
            root.display()
        );
        let (_, out) = run(&root, &payload, Some(FAIL));
        let _ = std::fs::remove_dir_all(&root);
        assert!(denied(&out), "an MCP git-commit tool must be checked: {out}");
    }

    #[test]
    fn an_mcp_pull_request_tool_is_submitted_by_name_shape() {
        let root = scratch();
        for tool in [
            "mcp__forge__create_pr",
            "mcp__forge__pr-create",
            "mcp__gh__create_pull_request",
        ] {
            let payload = format!(
                r#"{{"tool_name":"{tool}","tool_input":{{"body":"x","cwd":"{}"}}}}"#,
                root.display()
            );
            let (_, out) = run(&root, &payload, Some(FAIL));
            assert!(denied(&out), "{tool} must be checked: {out}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn without_jq_the_gate_denies_rather_than_allowing_everything() {
        // Every field the gate reads comes from jq. With jq missing the old
        // shape read every field as empty, DOMAIN stayed empty, and the gate
        // allowed everything: success reported, nothing enforced. It must
        // refuse instead, and say why.
        let root = scratch();
        let payload = format!(
            r#"{{"tool_name":"Bash","tool_input":{{"command":"cd {} && git commit -m x"}}}}"#,
            root.display()
        );
        let (_, out) = run_env(&root, &payload, Some(FAIL), false);
        let _ = std::fs::remove_dir_all(&root);
        assert!(denied(&out), "a jq-less environment must deny, not pass everything: {out}");
        assert!(out.contains("jq"), "the refusal names the missing tool: {out}");
    }

    #[test]
    fn a_heredoc_body_is_visible_because_the_whole_input_is_scanned() {
        // The predecessor's `[^"]*` extraction stopped at the first quote, so a
        // heredoc hid the trailer entirely.
        let root = scratch();
        let payload = format!(
            r#"{{"tool_name":"Bash","tool_input":{{"command":"cd {} && git commit -F - <<'EOF'\nfeat: x\n\nCo-Authored-By: Claude <a@b>\nEOF"}}}}"#,
            root.display()
        );
        let (_, out) = run(&root, &payload, Some(FAIL));
        let _ = std::fs::remove_dir_all(&root);
        assert!(denied(&out), "a heredoc body must be checked: {out}");
    }

    #[test]
    fn a_body_passed_by_file_is_read_and_submitted() {
        // Writing the body to a file and passing `--body-file` was a complete
        // bypass, since the predecessor only ever looked at the command string.
        let root = scratch();
        let body = root.join("body.md");
        std::fs::write(&body, "## Summary\n\nGenerated with [Claude Code](x)\n").unwrap();
        let payload = format!(
            r#"{{"tool_name":"Bash","tool_input":{{"command":"cd {} && gh pr create --body-file {}"}}}}"#,
            root.display(),
            body.display()
        );
        let (_, out) = run(&root, &payload, Some(FAIL));
        let _ = std::fs::remove_dir_all(&root);
        assert!(denied(&out), "a --body-file body must be read: {out}");
    }

    // --- the allow paths, so the hook is not simply denying everything ---

    #[test]
    fn an_unrelated_tool_is_left_alone() {
        // Matching every tool only works if irrelevant ones pass untouched;
        // otherwise the hook would block all work.
        let root = scratch();
        let payload = format!(
            r#"{{"tool_name":"Read","tool_input":{{"file_path":"{}/x.rs"}}}}"#,
            root.display()
        );
        let (_, out) = run(&root, &payload, Some(FAIL));
        let _ = std::fs::remove_dir_all(&root);
        assert!(!denied(&out), "an unrelated tool must not be denied: {out}");
    }

    #[test]
    fn an_unrelated_bash_command_is_left_alone() {
        let root = scratch();
        let payload = format!(
            r#"{{"tool_name":"Bash","tool_input":{{"command":"cd {} && git status"}}}}"#,
            root.display()
        );
        let (_, out) = run(&root, &payload, Some(FAIL));
        let _ = std::fs::remove_dir_all(&root);
        assert!(!denied(&out), "git status must not be denied: {out}");
    }

    #[test]
    fn a_passing_verdict_allows_the_tool() {
        let root = scratch();
        let payload = format!(
            r#"{{"tool_name":"Bash","tool_input":{{"command":"cd {} && git commit -m 'feat: x'"}}}}"#,
            root.display()
        );
        let (_, out) = run(&root, &payload, Some(PASS));
        let _ = std::fs::remove_dir_all(&root);
        assert!(!denied(&out), "a passing verdict must allow: {out}");
    }

    #[test]
    fn it_fails_closed_with_no_launcher() {
        // Consistent with every other anomalous state: refuse and say how to fix
        // it, rather than guessing a weaker policy.
        let root = scratch();
        let payload = format!(
            r#"{{"tool_name":"Bash","tool_input":{{"command":"cd {} && git commit -m 'feat: x'"}}}}"#,
            root.display()
        );
        let (_, out) = run(&root, &payload, None);
        let _ = std::fs::remove_dir_all(&root);
        assert!(denied(&out), "no launcher must fail closed: {out}");
        assert!(out.contains("cargo install cargo-mock"), "and say how: {out}");
    }
}
