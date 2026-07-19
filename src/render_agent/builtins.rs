#![allow(unused_imports)]
use super::*;

/// Generate the check-byline.sh hook content (without HOOK_HELPERS substitution).
///
/// Enforces the byline policy configured in `mock/agent/config.toml` under
/// `[attribution]`. Both `non_autonomous` and `autonomous` are glob patterns
/// (empty = no byline permitted / autonomous-mode config error).
pub(crate) fn builtin_check_byline(cfg: &Config) -> String {
    let mode_var = agent_mode_var(&cfg.project_name);
    let non_auto = bash_literal(&cfg.attribution.non_autonomous);
    let auto = bash_literal(&cfg.attribution.autonomous);
    format!(
        r##"#!/usr/bin/env bash
# Built-in mockspace hook: enforce commit authorship policy.
# Patterns come from mock/agent/config.toml [attribution]; baked at generation.
set -uo pipefail
__INPUT=$(cat)
{{{{HOOK_HELPERS}}}}
FILE_PATH=""
COMMAND=$(_extract "command")
_scope_or_allow

# Only check git commit commands.
if ! echo "$COMMAND" | grep -q "git commit"; then allow; fi

# Configured byline patterns (baked at generation time).
NON_AUTO_PATTERN={non_auto}
AUTO_PATTERN={auto}

AGENT_MODE="${{{mode_var}:-assistant}}"

# Extract all Co-Authored-By bylines from the command.
# Matches the trailer case-insensitively up to the closing double-quote of the
# git commit -m "..." argument (the typical shape). Trims prefix + whitespace.
# Limitation: if -m uses single quotes or HEREDOC the extraction may over-match;
# the git commit-msg hook catches those at real commit time.
BYLINES=$(printf '%s' "$COMMAND" \
    | grep -oiE 'co-authored-by:[^"]*' \
    | sed -E 's/^[Cc]o-[Aa]uthored-[Bb]y:[[:space:]]*//' \
    | sed -E 's/[[:space:]]+$//')

if [[ "$AGENT_MODE" == "autonomous" ]]; then
    if [[ -z "$AUTO_PATTERN" ]]; then
        deny "Autonomous mode is enabled but mock/agent/config.toml [attribution] autonomous is empty. Configure the expected byline pattern."
    fi
    if [[ -z "$BYLINES" ]]; then
        deny "Autonomous mode requires a Co-Authored-By byline matching: $AUTO_PATTERN"
    fi
    MATCHED=false
    while IFS= read -r line; do
        [[ -z "$line" ]] && continue
        if [[ "$line" == $AUTO_PATTERN ]]; then
            MATCHED=true
            break
        fi
    done <<< "$BYLINES"
    if [[ "$MATCHED" != "true" ]]; then
        deny "No Co-Authored-By byline matched autonomous pattern: $AUTO_PATTERN"
    fi
else
    # Non-autonomous (assistant) mode.
    if [[ -z "$NON_AUTO_PATTERN" ]]; then
        # Empty pattern: no byline permitted.
        if [[ -n "$BYLINES" ]]; then
            deny "Non-autonomous mode forbids Co-Authored-By bylines. Remove the byline, or set {mode_var}=autonomous if this is genuinely autonomous work."
        fi
    else
        # Non-empty pattern: bylines must match it.
        while IFS= read -r line; do
            [[ -z "$line" ]] && continue
            if [[ "$line" != $NON_AUTO_PATTERN ]]; then
                deny "Co-Authored-By byline does not match non-autonomous pattern ($NON_AUTO_PATTERN): $line"
            fi
        done <<< "$BYLINES"
    fi
fi
allow
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
    if echo "$REL_PATH" | grep -qE 'SHAME\.md\.tmpl$'; then allow; fi
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

# Gate broken: validator cleaned away, or core.hooksPath not wired. Self-heal
# by rebuilding (build.rs re-runs the bootstrap and re-activates), then retry.
( cd "$root/$mockdir" && cargo check --quiet --locked ) >/dev/null 2>&1 || true
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

