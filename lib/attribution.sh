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
# **Then deny by what an agent carries, for the surfaces that have no shape.** An
# author field and a committer field are not trailers and cannot be tested
# structurally, so those are read for a mailbox, a marker or a name that is a
# tool's own, and an advert in prose gets the enumeration of domains and
# mailboxes. Both go stale and that is tolerable, because they are the second net
# rather than the only one.
#
# The sets below are therefore a convenience and never the guarantee. A reader
# adding a tool to one is improving the second net; a reader relying on it alone
# has misread which net does the work.
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
# The identity net, for surfaces with no shape to test: an author, a committer,
# the halves of one a command gives.
#
# It recognises what only an agent carries, and never a word inside somebody's
# name. The first version was an unanchored substring regex over the whole line,
# and as a blocking check it refused Devin Smith, Hubbard Jones (bard), Haider Ali
# (aider), Cody Brown, Claude Monet, Anders Android (droid), Jules Verne, Mistral
# Winds and Ada Lombard on every commit they made, found by probe. What an agent
# carries is one of three things:
#
#   - a bot marker, in the name or the mailbox, which nobody writes into either
#     by accident;
#   - a mailbox it commits from, a glob over the whole mailbox. A vendor's domain
#     is not one, so a person writing from `anthropic.com` is a person;
#   - a name that is wholly a tool's own: optionally behind a vendor word, and
#     followed by nothing but the words that ride along with a tool (a model, a
#     product, a surface) and versions. `Claude Opus 4.1`, `GitHub Copilot` and
#     `Gemini Code Assist` are the tools, `Claude Monet`, `Max Claude` and `Claude
#     Max` are people: a companion counts only after the tool, and `max` is not
#     one. A group in parentheses is read on its own, as a tool's name when it is
#     one (`Jane Smith (aider)`) and left out when it is not (`Claude 3.5 Sonnet
#     (new)`).
#
# A tool whose name is also somebody's given name counts only with a second
# signal: a vendor word before it, a companion or a version after it, or one of
# the mailboxes. So `Claude Code <noreply@anthropic.com>` is the tool, and a
# person whose whole name is `Claude` or `Devin` is a person. The edge that is
# left is a tool that is not a given name and is somebody's whole name.
#
# Each set is a pipe-delimited string, so a membership test is one pattern match
# and the function adds no process to the hook that calls it: it keeps its words
# in a global array and holds no command substitution. The lists and the verdicts
# are the conformance table in `lint-rules/data/agent_identity_conformance.tsv`,
# which this library's suite and the lint pack's both read, and a list that
# drifts from it fails the suite that notices first.
# -----------------------------------------------------------------------------
ATTRIBUTION_AGENT_MARKERS='|[bot]|'
ATTRIBUTION_AGENT_MAILBOXES='|noreply@anthropic.com|noreply@openai.com|copilot@github.com|cursoragent@cursor.com|agent@cursor.com|amp@ampcode.com|grok@x.ai|openhands@all-hands.dev|*-bot@*|'
ATTRIBUTION_AGENT_TOOLS='|copilot|chatgpt|gpt|codex|grok|xai|x ai|windsurf|codeium|tabnine|supermaven|codewhisperer|amazon q|antigravity|opencode|openhands|replit|ghostwriter|phind|deepseek|qwen|ollama|llama|aider|cline|roo code|roo cline|lovable|droid|sourcegraph|anysphere|cognition labs|jetbrains ai|blackbox ai|continue dev|augment code|augmentcode|bolt new|v0 dev|factory ai|swe agent|ai assistant|coding agent|llm agent|crush bot|goose bot|anthropic|openai|opus|sonnet|haiku|'
ATTRIBUTION_AGENT_GIVEN='|claude|devin|gemini|amp|cursor|jules|cody|bard|kiro|junie|'
ATTRIBUTION_AGENT_HEADS='|github|google|anthropic|openai|microsoft|amazon|aws|'
ATTRIBUTION_AGENT_COMPANIONS='|code|agent|ai|bot|assistant|assist|cli|app|coding|via|slack|web|desktop|ide|extension|connector|integration|autofix|review|reviewer|new|preview|beta|opus|sonnet|haiku|instant|flash|turbo|lite|thinking|mini|'

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
    for p in ATTRIBUTION_TRAILER_KEY_RE ATTRIBUTION_AGENT_MARKERS ATTRIBUTION_AGENT_MAILBOXES \
             ATTRIBUTION_AGENT_TOOLS ATTRIBUTION_AGENT_GIVEN ATTRIBUTION_AGENT_HEADS \
             ATTRIBUTION_AGENT_COMPANIONS \
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
    attribution_names_agent 'Co-authored-by: Copilot <a@b.c>' || {
        log_error "attribution: the identity net does not recognise a known tool"; return 1; }
    attribution_names_agent 'dependabot[bot] <a@b.c>' || {
        log_error "attribution: the identity net does not recognise a bot marker"; return 1; }
    attribution_names_agent 'Dev <noreply@anthropic.com>' || {
        log_error "attribution: the identity net does not recognise an agent mailbox"; return 1; }
    attribution_has_advert 'see https://claude.ai/code' || {
        log_error "attribution: the advert net does not recognise a known advert"; return 1; }
    # And one that must NOT match, because a net that matches everything also
    # never reports clean and is just as broken.
    if attribution_is_attribution_trailer 'docs: describe the copilot surface'; then
        log_error "attribution: the shape net reads a commit subject as a trailer"
        return 1
    fi
    if attribution_names_agent 'Claude Monet <monet@example.org>'; then
        log_error "attribution: the identity net reads a person as an agent"
        return 1
    fi
    if attribution_names_agent 'Claude <a@b.c>'; then
        log_error "attribution: the identity net reads a given name with no second signal as an agent"
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
# Whitespace and hashes come off the front first, in any order and any number,
# because the anchor was the hole. A byline behind one leading space failed the
# match and the scan reported nothing, and so did every spelling with a `#` in
# front of it. Measured on git 2.55.0: `git commit -F` stores
# `# # Co-Authored-By: ...` in the body verbatim while git's own trailer parser
# returns nothing for it, so a byline spelled that way passes the parser the
# callers use for commit trailers and passed this net as well. What comes off is
# every arrangement of those two rather than the spellings somebody thought of,
# since the two previous repairs of this shape in the commit-msg hook each named
# a spelling and the next spelling got through.
#
# Whitespace and `#` is the whole of the set here, and it is not the whole of
# what defeats an anchor: `>`, `//`, `-`, `|`, `*` and a leading dot each hide a
# byline from this net as well, and git stores all of them verbatim. Those stay
# in, because this net reads markdown as well as commit messages, where `>` opens
# a quotation and `-` opens a list, and a false refusal here costs somebody a
# rehoused repository rather than one reword. A caller whose surface is only a
# commit message can take them off before it calls, and a caller that wants that
# has to do it: nothing in here does it for one, and a caller that passes a line
# through untouched gets the narrow set above and no more.
#
# What the strip costs, and it is not nothing. `author` and `committer` are in
# the key pattern as bare words, since a git author field has no `-by` shape, so
# with the anchor gone an indented `author: Jane` reads as a trailer. That is a
# markdown code block indented by four spaces, or a YAML document, or a struct
# literal in a diff. `attribution_strip_quoted` knows a fence and an inline
# backtick and does not know an indented block, and the trailer path never calls
# it.
#
# What that costs is measured rather than argued, by the two scripts under
# `mock/research/sketches/the-anchorless-trailer-net-corpus/`, and the answer
# depends on which corpus is asked. Over commit messages, every commit reachable
# from every ref in this repository and in one private workspace repository,
# the widening newly matches nothing. Over tracked file content it newly matches
# three lines here, two of them a struct literal in
# `mock/crates/mockspace-core/src/io/ref_write.rs` whose fields are named
# `author` and `committer`. So the shape is real and somebody can go and read
# it, and nothing refuses over it today because the trailer path is called on
# commit messages and on markdown rather than on rust. Run the scripts rather
# than trusting the counts in this paragraph, which were true when they were
# taken. `it_reads_an_indented_author_line_as_a_trailer` is where the shape is
# written down, and it is a pin on today's answer rather than an endorsement.
#
# A quotation stays safe, because the convention here is to backtick the
# forbidden string and a leading backtick is not stripped. A heading spelling
# out a live key with a real value and no backticks does read as a trailer, and
# that is the trade: the cheaper repair is to backtick it.
#
# Usage: attribution_is_attribution_trailer "$line" && ...
#[pub]
attribution_is_attribution_trailer() {
    local line="${1:-}"
    line="${line#"${line%%[![:space:]#]*}"}"
    printf '%s' "$line" | grep -qiE "^(${ATTRIBUTION_TRAILER_KEY_RE}):[[:space:]]*[^[:space:]]"
}

# attribution_names_agent <identity>
#
# Whether an identity is an agent's, by what only an agent carries: a bot marker,
# a mailbox it commits from, or a name that is wholly a tool's own. The second
# net, for author and committer fields, which have no trailer shape to test. The
# comment on the sets above says what each means and why a word inside a person's
# name is none of them.
#
# Takes the forms a caller has one in, so none of them is the caller's to unpick:
# `Name <mailbox>`, the same with the date and zone `git var` appends, a trailer
# line (`Co-authored-by: Name <mailbox>`), what a command gives (`--author=...`,
# `GIT_AUTHOR_NAME=...`, `GIT_AUTHOR_EMAIL=...`), and a bare name or mailbox.
#
# Usage: attribution_names_agent "$identity" && ...
#[pub]
attribution_names_agent() {
    local text="${1:-}" name mailbox rest inner m entry
    local -a entries
    # leading whitespace
    text="${text#"${text%%[![:space:]]*}"}"
    # what a command wraps an identity in, and a trailer key
    if [[ "$text" =~ ^(--author[=[:space:]]+|GIT_(AUTHOR|COMMITTER)_(NAME|EMAIL)=)(.*)$ ]]; then
        text="${BASH_REMATCH[4]}"
    elif [[ "$text" =~ ^[A-Za-z]+(-[A-Za-z]+)*:[[:space:]]*(.*)$ ]]; then
        text="${BASH_REMATCH[2]}"
    fi
    # quotes round the whole of it
    if [[ "$text" =~ ^[\"\'](.*)[\"\']$ ]]; then
        text="${BASH_REMATCH[1]}"
    fi

    local angle='^(.*)[<]([^<>]*)[>].*$'
    if [[ "$text" =~ $angle ]]; then
        name="${BASH_REMATCH[1]}"
        mailbox="${BASH_REMATCH[2]}"
    elif [[ "$text" == *@* && "$text" != *[[:space:]]* ]]; then
        name=""
        mailbox="$text"
    else
        name="$text"
        mailbox=""
    fi
    name="${name,,}"
    mailbox="${mailbox,,}"

    IFS='|' read -ra entries <<< "$ATTRIBUTION_AGENT_MARKERS"
    for m in ${entries[@]+"${entries[@]}"}; do
        [[ -n "$m" && ( "$name" == *"$m"* || "$mailbox" == *"$m"* ) ]] && return 0
    done
    if [[ -n "$mailbox" ]]; then
        IFS='|' read -ra entries <<< "$ATTRIBUTION_AGENT_MAILBOXES"
        for entry in ${entries[@]+"${entries[@]}"}; do
            # the entry is a glob over the whole mailbox, so it is not quoted
            # shellcheck disable=SC2053
            [[ -n "$entry" && "$mailbox" == $entry ]] && return 0
        done
    fi

    # a group in parentheses is read on its own, as a tool's name when it is one
    # and left out of the name when it is not
    rest="$name"
    while [[ "$rest" =~ \(([^\(\)]*)\) ]]; do
        inner="${BASH_REMATCH[1]}"
        _attribution_words "$inner"
        _attribution_words_name_a_tool && return 0
        rest="${rest/"${BASH_REMATCH[0]}"/ }"
    done
    _attribution_words "$rest"
    _attribution_words_name_a_tool
}

# _attribution_words <text>
#
# The lowercase words of a text into `_ATTRIBUTION_WORDS`, split at anything that
# is not a letter or a digit. A global and not an output, so that nothing that
# reads it needs a command substitution and a hook judging every commit in a push
# forks nothing.
_attribution_words() {
    local text="${1,,}"
    text="${text//[^[:alnum:]]/ }"
    _ATTRIBUTION_WORDS=()
    read -ra _ATTRIBUTION_WORDS <<< "$text"
}

# _attribution_words_name_a_tool
#
# Whether the words in `_ATTRIBUTION_WORDS` are a tool's name: optionally behind
# vendor words, then a tool, then only companion words and versions.
_attribution_words_name_a_tool() {
    local n=${#_ATTRIBUTION_WORDS[@]} k=0
    ((n == 0)) && return 1
    while ((k < n)); do
        _attribution_tools_at "$k" && return 0
        # one more vendor word in front, or the words are not a tool's
        [[ "$ATTRIBUTION_AGENT_HEADS" == *"|${_ATTRIBUTION_WORDS[k]}|"* ]] || return 1
        k=$((k + 1))
    done
    return 1
}

# _attribution_tools_at <k>
#
# Whether a tool's name starts at word `k` and is followed by nothing but
# companions and versions, with the second signal a given-name tool wants.
_attribution_tools_at() {
    local k="$1" r entry after w ok seen_tail
    local -a entries
    r=" ${_ATTRIBUTION_WORDS[*]:k} "

    IFS='|' read -ra entries <<< "$ATTRIBUTION_AGENT_TOOLS"
    for entry in ${entries[@]+"${entries[@]}"}; do
        [[ -n "$entry" && "$r" == " $entry "* ]] || continue
        after="${r#" $entry "}"
        _attribution_tail_is_companions "$after" && return 0
    done

    IFS='|' read -ra entries <<< "$ATTRIBUTION_AGENT_GIVEN"
    for entry in ${entries[@]+"${entries[@]}"}; do
        [[ -n "$entry" && "$r" == " $entry "* ]] || continue
        after="${r#" $entry "}"
        _attribution_tail_is_companions "$after" || continue
        # the second signal: a vendor word before it, or a companion or version after
        if ((k > 0)) || [[ -n "${after// /}" ]]; then
            return 0
        fi
    done
    return 1
}

# _attribution_tail_is_companions <words>
#
# Whether every word is a companion or a version. No words is true.
_attribution_tail_is_companions() {
    local w
    for w in $1; do
        [[ "$ATTRIBUTION_AGENT_COMPANIONS" == *"|${w}|"* ]] && continue
        [[ "$w" == [0-9]* ]] && continue
        [[ "$w" =~ ^v[0-9]+$ ]] && continue
        return 1
    done
    return 0
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
# Both nets run over the whole text. This said the trailer net read the
# message's final block, which it has never done, and nothing was pinning the
# behaviour either way: `it_reports_a_byline_that_is_not_in_the_final_block` is
# what pins it now. A trailer does belong in the final block and a caller with a
# real trailer parser should use one, but a byline somebody put in the middle of
# a body is the case this is asked about, and a final-block scan answers clean
# on it.
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
