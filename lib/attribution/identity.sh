#!/usr/bin/env bash
# =============================================================================
# mockspace/attribution/identity.sh - the identity net: what only an agent carries
# =============================================================================
# Part of the attribution library, sourced by `lib/attribution.sh` and reached
# through `use mockspace::attribution` like the rest of it, not on its own.
# =============================================================================

# -----------------------------------------------------------------------------
# The identity net, for surfaces with no shape to test: an author, a committer,
# the halves of one a command gives.
#
# It recognises what only an agent carries, and never a word inside somebody's
# name. The first version was an unanchored substring regex over the whole line,
# and as a blocking check it refused Devin Smith, Hubbard Jones (bard), Haider Ali
# (aider), Cody Brown, Claude Monet, Anders Android (droid), Jules Verne, Mistral
# Winds and Ada Lombard on every commit they made, found by probe. What an agent
# carries is one of these, tried in this order:
#
#   - a bot marker, in the name or the mailbox, which nobody writes into either
#     by accident;
#   - a mailbox it commits from, a glob over the whole mailbox. A vendor's domain
#     is not one, so a person writing from `anthropic.com` is a person;
#   - a tag a tool writes into a name, which is all a group in parentheses is read
#     for. `Paul Gauthier (aider)` is the tool, `Jane Doe (OpenAI)` is Jane Doe,
#     and a group holding anything else is left out of the name, so `Claude 3.5
#     Sonnet (new)` is read as `Claude 3.5 Sonnet`;
#   - a name the caller says is an agent's, whatever the mailbox: the second
#     argument of `attribution_names_agent`, matched against the whole name;
#   - a name that starts with a tool's own, behind vendor words if any, and
#     followed by nothing but the words that ride along with a tool and versions:
#     `Copilot Chat`, `Cursor Bugbot`, `Claude Code on the web` and `GPT 4o` are
#     the tools. Any other word after it is a word of somebody's name, so
#     `Copilot Smith`, `Cline, John` and `Claude Monet` are people, and a tool
#     inside somebody's name is none (`Smith Copilot`).
#
# A tool alone is an agent, except where its name is also somebody's given name
# (Claude, Devin, Gemini and the rest of `ATTRIBUTION_AGENT_GIVEN`). Those want a
# second signal when nothing follows them, since a person can be called that: a
# vendor word in front (`Google Gemini`), or a mailbox that is the tool's. The
# mailbox is the tool's when its domain is one of the tool's vendor domains in
# `ATTRIBUTION_AGENT_VENDORS`, matched whole, or when its local part is the tool's
# word at a mailbox that is a machine's: a domain that is `localhost`, one label
# with no dot, or one ending in one of `ATTRIBUTION_AGENT_MACHINES`. The same
# local part at any other domain is a person called that, `Devin <devin@acme.com>`,
# and so is GitHub's private address, `Devin <12345+devin@users.noreply.github.com>`,
# which every account has.
#
# What the default lets through is a bare given name behind a mailbox that is
# neither the tool's nor on the list: `claude <root@buildhost.local>`, which was
# found, and a person's own `Devin <devin@acme.com>`, which has to stay a person.
# A caller that knows its own build hosts passes the names, and only a name it
# passes is read that way.
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
ATTRIBUTION_AGENT_TAGS='|aider|'
ATTRIBUTION_AGENT_TOOLS='|copilot|chatgpt|gpt|codex|grok|xai|x ai|windsurf|codeium|tabnine|supermaven|codewhisperer|amazon q|antigravity|opencode|openhands|replit|ghostwriter|phind|deepseek|qwen|ollama|llama|aider|cline|roo code|roo cline|lovable|droid|sourcegraph|anysphere|cognition labs|jetbrains ai|blackbox ai|continue dev|augment code|augmentcode|bolt new|v0 dev|factory ai|swe agent|ai assistant|coding agent|llm agent|crush bot|goose bot|anthropic|openai|opus|sonnet|haiku|cursor|'
ATTRIBUTION_AGENT_GIVEN='|claude|devin|gemini|amp|jules|cody|bard|kiro|junie|'
ATTRIBUTION_AGENT_VENDORS='|claude anthropic.com|claude claude.com|claude claude.ai|devin cognition.ai|devin devin.ai|gemini google.com|amp ampcode.com|jules jules.google|cody sourcegraph.com|bard google.com|kiro kiro.dev|junie jetbrains.com|'
ATTRIBUTION_AGENT_HEADS='|github|google|anthropic|openai|microsoft|amazon|aws|'
ATTRIBUTION_AGENT_MACHINES='|local|localdomain|lan|internal|home.arpa|'
ATTRIBUTION_AGENT_COMPANIONS='|code|agent|ai|bot|assistant|assist|cli|app|coding|via|slack|web|desktop|ide|extension|connector|integration|autofix|review|reviewer|new|preview|beta|opus|sonnet|haiku|instant|flash|turbo|lite|thinking|mini|chat|workspace|bugbot|action|sdk|on|the|'

# attribution_names_agent <identity> [names]
#
# Whether an identity is an agent's, by what only an agent carries: a bot marker,
# a mailbox it commits from, a tag a tool writes into a name, a name the caller
# passes, or a name that starts with a tool's own. The second net, for author and
# committer fields, which have no trailer shape to test. The comment on the sets
# above says what each means and why a word inside a person's name is none of them.
#
# The second argument is the names the caller says are agents whatever the
# mailbox, separated by `|`, `,` or a line break (`Claude|Devin`). A name is
# matched against the whole name of the identity, a group in parentheses left out,
# so `Claude` names `claude <root@buildhost.local>` and does not name `Claude
# Monet`. Without it, a bare given name behind a mailbox that is not the tool's is
# a person's, and that is what the default lets through.
#
# Takes the forms a caller has one in, so none of them is the caller's to unpick:
# `Name <mailbox>`, the same with the date and zone `git var` appends, a trailer
# line (`Co-authored-by: Name <mailbox>`), what a command gives (`--author=...`,
# `GIT_AUTHOR_NAME=...`, `GIT_AUTHOR_EMAIL=...`), and a bare name or mailbox.
#
# Usage: attribution_names_agent "$identity" ["Claude|Devin"] && ...
#[pub]
attribution_names_agent() {
    local text="${1:-}" names="${2:-}" name mailbox rest group m entry
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

    # Who the mailbox is, for a tool that is also a given name: the local part and
    # the domain. A person is called the same thing at an ordinary domain, so the
    # local part is a signal at a machine's mailbox and nowhere else.
    _ATTRIBUTION_LOCAL=""
    _ATTRIBUTION_DOMAIN=""
    if [[ "$mailbox" == *@* ]]; then
        _ATTRIBUTION_LOCAL="${mailbox%@*}"
        _ATTRIBUTION_DOMAIN="${mailbox##*@}"
    fi

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

    # A group in parentheses is read for a tag and for nothing else, and what is
    # not a tag is left out of the name.
    rest="$name"
    while [[ "$rest" =~ \(([^\(\)]*)\) ]]; do
        group="${BASH_REMATCH[0]}"
        _attribution_words "${BASH_REMATCH[1]}"
        _attribution_words_are_a_tag && return 0
        rest="${rest/"$group"/ }"
    done

    _attribution_words "$rest"
    if [[ -n "$names" ]]; then
        _attribution_words_are_a_named_agent "$names" && return 0
        _attribution_words "$rest"
    fi
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

# _attribution_words_are_a_tag
#
# Whether the words in `_ATTRIBUTION_WORDS` are exactly a tag a tool writes into
# a name.
_attribution_words_are_a_tag() {
    local IFS=' ' joined
    joined="${_ATTRIBUTION_WORDS[*]}"
    [[ -n "$joined" && "$ATTRIBUTION_AGENT_TAGS" == *"|${joined}|"* ]]
}

# _attribution_words_are_a_named_agent <names>
#
# Whether the words in `_ATTRIBUTION_WORDS` are exactly one of the names the
# caller passed. Leaves `_ATTRIBUTION_WORDS` holding the last name it compared, so
# a caller that still wants the words reads them again.
_attribution_words_are_a_named_agent() {
    local IFS=' ' key entry
    local -a entries
    key="${_ATTRIBUTION_WORDS[*]}"
    [[ -n "$key" ]] || return 1
    # `read -d ''` reaches the end of the input without a delimiter and says so
    IFS=$'|,\n' read -d '' -ra entries <<< "$1" || true
    for entry in ${entries[@]+"${entries[@]}"}; do
        _attribution_words "$entry"
        [[ "${_ATTRIBUTION_WORDS[*]}" == "$key" ]] && return 0
    done
    return 1
}

# _attribution_words_name_a_tool
#
# Whether the words in `_ATTRIBUTION_WORDS` start with a tool's name, optionally
# behind vendor words.
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
# Whether a tool's name starts at word `k`, with nothing after it but companions
# and versions. A tool that is not a given name is an agent alone. One that is
# wants, alone, a vendor word before it or a mailbox that is the tool's.
_attribution_tools_at() {
    local IFS=' ' k="$1" r entry after
    local -a entries
    r=" ${_ATTRIBUTION_WORDS[*]:k} "

    IFS='|' read -ra entries <<< "$ATTRIBUTION_AGENT_TOOLS"
    for entry in ${entries[@]+"${entries[@]}"}; do
        [[ -n "$entry" && "$r" == " $entry "* ]] || continue
        after="${r#" $entry "}"
        # the tool alone, or followed by words that ride along with one
        if [[ -z "${after// /}" ]] || _attribution_tail_names_a_tool "$after"; then
            return 0
        fi
    done

    IFS='|' read -ra entries <<< "$ATTRIBUTION_AGENT_GIVEN"
    for entry in ${entries[@]+"${entries[@]}"}; do
        [[ -n "$entry" && "$r" == " $entry "* ]] || continue
        after="${r#" $entry "}"
        if [[ -z "${after// /}" ]]; then
            # the tool alone: a vendor word before it, or a mailbox that is its own
            ((k > 0)) && return 0
            _attribution_mailbox_is_the_tools "$entry" && return 0
        else
            _attribution_tail_names_a_tool "$after" && return 0
        fi
    done
    return 1
}

# _attribution_tail_names_a_tool <words>
#
# Whether the words after a tool are nothing but companions and versions. A word
# that is neither is somebody's, and so is the whole of what follows.
_attribution_tail_names_a_tool() {
    local IFS=' ' w
    for w in $1; do
        [[ "$ATTRIBUTION_AGENT_COMPANIONS" == *"|${w}|"* ]] && continue
        _attribution_is_a_version "$w" && continue
        return 1
    done
    return 0
}

# _attribution_is_a_version <word>
#
# A word opening with a digit, or `v` and digits.
_attribution_is_a_version() {
    [[ "$1" == [0-9]* ]] && return 0
    [[ "$1" =~ ^v[0-9]+$ ]]
}

# _attribution_mailbox_is_the_tools <tool>
#
# Whether the mailbox of the identity being judged is the given-name tool's: its
# domain is one of the tool's vendor domains, matched whole, or its local part is
# the tool's word at a mailbox that is a machine's. Reads `_ATTRIBUTION_LOCAL` and
# `_ATTRIBUTION_DOMAIN`, which `attribution_names_agent` sets, and a mailbox with
# no domain is nobody's.
_attribution_mailbox_is_the_tools() {
    local tool="$1" entry
    local -a entries
    [[ -n "$_ATTRIBUTION_DOMAIN" ]] || return 1
    [[ "$_ATTRIBUTION_LOCAL" == "$tool" ]] && _attribution_domain_is_a_machines && return 0
    IFS='|' read -ra entries <<< "$ATTRIBUTION_AGENT_VENDORS"
    for entry in ${entries[@]+"${entries[@]}"}; do
        [[ "$entry" == "$tool "* && "$_ATTRIBUTION_DOMAIN" == "${entry#* }" ]] && return 0
    done
    return 1
}

# _attribution_domain_is_a_machines
#
# Whether `_ATTRIBUTION_DOMAIN` is a machine's: `localhost` or any domain of one
# label, or one ending in one of `ATTRIBUTION_AGENT_MACHINES`. Nothing that is not
# a mailbox with a domain is a machine's.
_attribution_domain_is_a_machines() {
    local entry
    local -a entries
    [[ -n "$_ATTRIBUTION_DOMAIN" ]] || return 1
    [[ "$_ATTRIBUTION_DOMAIN" == *.* ]] || return 0
    IFS='|' read -ra entries <<< "$ATTRIBUTION_AGENT_MACHINES"
    for entry in ${entries[@]+"${entries[@]}"}; do
        [[ -n "$entry" && "$_ATTRIBUTION_DOMAIN" == *".${entry}" ]] && return 0
    done
    return 1
}
