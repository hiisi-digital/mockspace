#!/usr/bin/env bash
# =============================================================================
# mockspace/attribution.sh - the engine for agent bylines and tool adverts
# =============================================================================
# Part of mockspace. https://github.com/hiisi-digital/mockspace
#
# Reached from a consumer as:
#
#   [deps.mockspace]                    # in the unit's nut.toml
#   git = "https://github.com/hiisi-digital/mockspace.git"
#   ref = "dev"
#
#   use mockspace::attribution          # in the script
#
# This is the engine and it holds no policy. What counts as an attribution, what
# counts as an advert, and where each may be matched are answered here. Whether
# a given byline is permitted is answered by the caller, which passes an allow
# pattern in. mockspace reads that from `[attribution]` in its own agent config;
# a caller's own review sweep passes its own. Neither shape belongs here.
#
# It exists because there were three implementations of this and they disagreed.
# One knew seven vendors, one knew a single vendor, one was a stale fork of the
# first left behind when a script moved. A byline that any of them would have
# caught reached a collection branch, because the narrow one was the one that
# ran and the broad one was invoked by nothing.
#
# ## Two nets, and the first one is the one that keeps working
#
# **Deny by shape, not by vendor.** A trailer whose key is an attribution key is
# a finding unless the caller's allow pattern accepts it. That net does not know
# what Claude or Copilot or Codex are and does not need to: a tool shipping next
# year with a name nobody here has heard is caught on the day it ships, because
# the question asked is "is this line attributing the work to somebody" and not
# "is this line one of the forty vendors I remembered".
#
# **Then deny by vendor, for the surfaces that have no shape.** An author field,
# a committer field and an advert in prose are not trailers and cannot be tested
# structurally, so those get the enumeration. It goes stale and that is
# tolerable, because it is the second net rather than the only one.
#
# The enumeration below is therefore a convenience and never the guarantee. A
# reader adding a vendor to it is improving the second net; a reader relying on
# it alone has misread which net does the work.
#
# ## Fail closed
#
# Every entry point refuses rather than reporting clean when it cannot do its
# job. A scanner that examines nothing finds nothing, and finding nothing is
# what a clean repository also looks like, so the two must not share an exit
# code. `attribution_selfcheck` is the assertion that the patterns loaded at
# all, and callers run it before trusting a clean verdict.
# =============================================================================

nut_once || return 0

use log

# -----------------------------------------------------------------------------
# The shape net: which `Key: value` lines attribute work to somebody.
#
# `*-by` covers `Co-authored-by`, `Signed-off-by`, `Assisted-by`, `Reviewed-by`
# and whatever the next one is called. The rest are keys tools have actually
# shipped.
#
# A conventional-commit subject is `Key: value` shaped too, and matching one
# would condemn a repository for a commit about a feature: `docs: describe how
# the copilot integration surface is configured` is not a trailer. So the key
# must be an attribution key rather than merely a word, and the caller should
# prefer a real trailer parser where it has one.
# -----------------------------------------------------------------------------
ATTRIBUTION_TRAILER_KEY_RE='([A-Za-z]+-)*[A-Za-z]+-(by|session|agent|model|tool)|author|committer|generated-with'

# -----------------------------------------------------------------------------
# The vendor net, for surfaces with no shape to test. Grouped by who ships them
# rather than alphabetically, so a reader adding one can see whether the family
# is already covered.
# -----------------------------------------------------------------------------
ATTRIBUTION_IDENTITY_RE='(claude|anthropic|copilot|githubcopilot|codex|chatgpt|openai|gpt-[0-9]|cursor|anysphere|gemini|bard|jules|devin|cognition[- ]?labs|aider|windsurf|codeium|cody|sourcegraph|ampcode|amp-?bot|tabnine|supermaven|augment(code)?|codewhisperer|amazon[- ]?q|kiro|replit|ghostwriter|phind|blackbox\.ai|grok|x\.ai|xai|deepseek|qwen|mistral|llama|ollama|junie|jetbrains[- ]?ai|continue\.dev|zed[- ]?industries|v0\.dev|lovable|bolt\.new|factory\.ai|droid|opencode|crush[- ]?bot|goose[- ]?bot|roo[- ]?(code|cline)|cline|antigravity|\[bot\]|-bot@|bot@users\.noreply|ai[- ]assistant|coding[- ]agent|llm[- ]agent)'

# -----------------------------------------------------------------------------
# Adverts: the tool-promotion suffixes platforms bake into their defaults.
#
# Domains, mailboxes and the robot emoji, which have no innocent reading in a
# commit message. Deliberately not bare English.
# -----------------------------------------------------------------------------
ATTRIBUTION_ADVERT_URL_RE='(claude\.com/claude-code|claude\.ai/code|noreply@anthropic\.com|github\.com/features/copilot|copilot@github\.com|noreply@github\.com|openai\.com/(codex|chatgpt)|chatgpt\.com|noreply@openai\.com|cursor\.(com|sh|so)|codeium\.com|windsurf\.com|sourcegraph\.com/cody|ampcode\.com|devin\.ai|cognition\.ai|aider\.chat|gemini\.google\.com|jules\.google|tabnine\.com|supermaven\.com|augmentcode\.com|replit\.com/ai|phind\.com|blackbox\.ai|v0\.dev|lovable\.dev|bolt\.new|factory\.ai|continue\.dev|🤖)'

# The "generated with" family, anchored.
#
# An earlier scanner matched these phrases bare, anywhere in a message. It
# reported a repository as contaminated on the strength of
#
#     fix: accept a placeholder written with spaces inside its braces
#
# and reported its own rule documentation as a violation of itself. A check
# whose false positives condemn a repository is worse than no check, because the
# remedy it triggers is irreversible. So the phrase must be followed, within a
# short window, by something that is actually a tool: a markdown link, a URL or
# an angle-bracketed address.
ATTRIBUTION_ADVERT_PHRASE_RE='(generated|created|written|authored|made|built)[[:space:]]+(with|by)[[:space:]]*:?[[:space:]]*(\[|https?://|<)'

ATTRIBUTION_ADVERT_RE="(${ATTRIBUTION_ADVERT_URL_RE}|${ATTRIBUTION_ADVERT_PHRASE_RE})"

# attribution_selfcheck
#
# Assert the engine loaded. Every pattern non-empty, and each one matching a
# string it must match, so that a truncated or half-sourced library cannot pass
# for a working one.
#
# Callers run this before reporting a clean verdict. A scan that examined
# nothing and a repository that contains nothing look identical from outside,
# and this is what tells them apart.
#
# Usage: attribution_selfcheck || exit 2
#[pub]
attribution_selfcheck() {
    local p
    for p in ATTRIBUTION_TRAILER_KEY_RE ATTRIBUTION_IDENTITY_RE \
             ATTRIBUTION_ADVERT_URL_RE ATTRIBUTION_ADVERT_PHRASE_RE ATTRIBUTION_ADVERT_RE; do
        if [[ -z "${!p:-}" ]]; then
            log_error "attribution: ${p} is empty; refusing to report clean"
            return 1
        fi
    done
    # One canary per net. If these stop matching, the engine is broken in a way
    # that would otherwise read as "this repository is clean".
    attribution_is_attribution_trailer 'Co-authored-by: someone <a@b.c>' || {
        log_error "attribution: the shape net does not recognise a trailer"; return 1; }
    attribution_names_agent 'Co-authored-by: Claude <a@b.c>' || {
        log_error "attribution: the vendor net does not recognise a known vendor"; return 1; }
    attribution_has_advert 'see https://claude.ai/code' || {
        log_error "attribution: the advert net does not recognise a known advert"; return 1; }
    # And one that must NOT match, because a net that matches everything also
    # never reports clean and is just as broken.
    if attribution_is_attribution_trailer 'docs: describe the copilot surface'; then
        log_error "attribution: the shape net reads a commit subject as a trailer"
        return 1
    fi
    return 0
}

# attribution_is_attribution_trailer <line>
#
# Whether a line is structurally an attribution: an attribution key, a colon,
# and a value. Says nothing about who is named or whether it is permitted.
#
# This is the net that keeps working when a new tool ships, so a caller wanting
# one check should want this one.
#
# Usage: attribution_is_attribution_trailer "$line" && ...
#[pub]
attribution_is_attribution_trailer() {
    printf '%s' "${1:-}" | grep -qiE "^(${ATTRIBUTION_TRAILER_KEY_RE}):[[:space:]]*[^[:space:]]"
}

# attribution_names_agent <line>
#
# Whether a line names a vendor the enumeration knows. The second net, for
# author and committer fields, which have no trailer shape to test.
#
# Usage: attribution_names_agent "$line" && ...
#[pub]
attribution_names_agent() {
    printf '%s' "${1:-}" | grep -qiE "${ATTRIBUTION_IDENTITY_RE}"
}

# attribution_strip_quoted
#
# Filter. Drops fenced blocks and inline backticks from stdin.
#
# This workspace contains rules and documentation that quote forbidden strings
# in order to forbid them, and a scanner that cannot tell a rule from a
# violation is a scanner that condemns the rule.
#
# A code span may wrap, so the inline strip runs over a paragraph rather than
# over a line. Prose wraps and a commit body wraps hardest, so the quotation
# this exists to spare arrives split as often as not, and a line-oriented strip
# sees an opening backtick with no closing one and leaves the span standing.
#
# The paragraph is the bound and it is deliberate. Joining the whole message
# instead would let one unbalanced backtick swallow everything up to the next
# one anywhere later, hiding a real suffix behind a stray quote written
# paragraphs above it. A blank line closes a span whatever the backticks are
# doing, and a tool's suffix sits in its own paragraph, which is what keeps the
# repair from buying a hole. Both directions are pinned in the suite.
#
# The lines inside a paragraph stay lines. The span strip already crosses a
# line break on its own, since a paragraph is one record, and joining the
# lines besides would let the phrase net read "generated by" on one line
# together with a link starting the next, which is ordinary prose and not a
# suffix. That direction is pinned too.
#
# Usage: printf '%s' "$text" | attribution_strip_quoted
#[pub]
attribution_strip_quoted() {
    sed -e '/^```/,/^```/d' | awk '
        BEGIN { RS = ""; ORS = "\n\n" }
        { gsub(/`[^`]*`/, ""); print }
    '
}

# attribution_has_advert <text>
#
# Whether text carries a tool-promotion advert, ignoring anything quoted.
#
# Usage: attribution_has_advert "$body" && ...
#[pub]
attribution_has_advert() {
    printf '%s' "${1:-}" | attribution_strip_quoted | grep -qiE "${ATTRIBUTION_ADVERT_RE}"
}

# attribution_advert_excerpt <text>
#
# The first advert in text with a little context either side, for a report.
# Empty when there is none.
#
# Usage: excerpt=$(attribution_advert_excerpt "$body")
#[pub]
attribution_advert_excerpt() {
    printf '%s' "${1:-}" | attribution_strip_quoted \
        | grep -oiE ".{0,30}${ATTRIBUTION_ADVERT_RE}.{0,30}" | head -1
}

# attribution_allows <line> <allow-pattern>
#
# Whether the caller's policy permits this attribution line.
#
# The pattern is a bash glob, matched against the trailer's value. Empty permits
# nothing, which is the right default: a policy that has not been stated should
# refuse rather than allow, so a missing config fails toward refusing a byline.
#
# The engine holds no policy and never consults a config. mockspace reads
# `[attribution]` from its agent config and passes the field; another consumer
# passes whatever it uses.
#
# Usage: attribution_allows "$line" "$pattern" || finding "$line"
#[pub]
attribution_allows() {
    local line="${1:-}" pattern="${2:-}" value
    [[ -z "$pattern" ]] && return 1
    value="${line#*:}"
    # Trimmed here rather than with a library call. One fewer thing that can be
    # missing, and a missing helper in this engine exits 127, which every
    # `assert_fails` in a caller's suite would read as a correct refusal.
    value="${value#"${value%%[![:space:]]*}"}"
    value="${value%"${value##*[![:space:]]}"}"
    # shellcheck disable=SC2053
    [[ "$value" == $pattern ]]
}

# attribution_scan_message <text> [allow-pattern]
#
# Every finding in one commit message or pull-request body, one per line, as
# `kind<TAB>excerpt`. Kinds are `trailer` and `advert`.
#
# The trailer net runs over the message's final block, which is where a trailer
# lives; the advert net runs over the whole text, since an advert is a suffix
# somebody's tooling appended and may sit anywhere.
#
# Usage: while IFS=$'\t' read -r kind hit; do ...; done < <(attribution_scan_message "$b")
#[pub]
attribution_scan_message() {
    local text="${1:-}" allow="${2:-}" line
    while IFS= read -r line; do
        [[ -z "$line" ]] && continue
        attribution_is_attribution_trailer "$line" || continue
        attribution_allows "$line" "$allow" && continue
        printf 'trailer\t%s\n' "$line"
    done < <(printf '%s\n' "$text")

    local hit
    hit="$(attribution_advert_excerpt "$text")"
    [[ -n "$hit" ]] && printf 'advert\t%s\n' "$hit"
    return 0
}
