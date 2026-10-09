#!/usr/bin/env bash
# shellcheck shell=bash
# =============================================================================
# identity_table - the conformance table and the library, loaded for the suites
# =============================================================================
# Sourced by `attribution_identity_*_test.sh`, and named so that `./test` does
# not run it as a suite of its own. It loads the library, checks that the
# functions the suites lean on exist, and reads the table into the `T_` arrays.
# =============================================================================

# Sourced by path for the reason `attribution_test.sh` gives.
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/lib/attribution.sh"

# A missing function exits 127, and `assert_fails` reads that as a correct
# refusal, so existence is asserted once, here, and not inferred from behaviour.
for _fn in attribution_names_agent attribution_is_attribution_trailer attribution_has_advert \
           _attribution_words _attribution_words_are_a_tag _attribution_words_are_a_named_agent \
           _attribution_words_name_a_tool _attribution_tools_at _attribution_tail_names_a_tool \
           _attribution_is_a_version _attribution_mailbox_is_the_tools \
           _attribution_domain_is_a_machines; do
    if ! declare -F "$_fn" >/dev/null; then
        printf 'identity_table: %s is not defined; the library did not load\n' "$_fn" >&2
        printf 'every assert_fails below would pass on exit 127 and mean nothing\n' >&2
        exit 2
    fi
done
unset _fn

# --- the table ----------------------------------------------------------------

# Read the conformance table into the `T_` arrays. A line that is not a comment,
# not blank and not a row of a known kind is an error, said and returned as 2,
# since a row that silently fails to count makes a suite over the table pass for
# less than it says. A kind that pairs two things takes both, and a kind that
# holds one takes no second.
_read_conformance() {
    local file="$1" kind value second at=0
    T_marker=() T_mailbox=() T_tag=() T_tool=() T_given=() T_vendor=() T_machine=()
    T_head=() T_companion=() T_keyed_name=() T_keyed_identity=() T_person=() T_agent=()
    while IFS=$'\t' read -r kind value second || [[ -n "$kind" ]]; do
        at=$((at + 1))
        [[ -z "$kind" || "$kind" == \#* ]] && continue
        if [[ -z "$value" ]]; then
            printf 'line %d: `%s` has no value\n' "$at" "$kind" >&2
            return 2
        fi
        case "$kind" in
            vendor|keyed)
                if [[ -z "$second" || "$second" == *$'\t'* ]]; then
                    printf 'line %d: `%s` wants two values\n' "$at" "$kind" >&2
                    return 2
                fi ;;
            *)
                if [[ -n "$second" ]]; then
                    printf 'line %d: `%s` takes one value\n' "$at" "$kind" >&2
                    return 2
                fi ;;
        esac
        case "$kind" in
            marker)    T_marker+=("$value") ;;
            mailbox)   T_mailbox+=("$value") ;;
            tag)       T_tag+=("$value") ;;
            tool)      T_tool+=("$value") ;;
            given)     T_given+=("$value") ;;
            vendor)    T_vendor+=("$value $second") ;;
            machine)   T_machine+=("$value") ;;
            head)      T_head+=("$value") ;;
            companion) T_companion+=("$value") ;;
            keyed)     T_keyed_name+=("$value"); T_keyed_identity+=("$second") ;;
            person)    T_person+=("$value") ;;
            agent)     T_agent+=("$value") ;;
            *) printf 'line %d: unknown kind `%s`\n' "$at" "$kind" >&2; return 2 ;;
        esac
    done < "$file"
}

_conformance="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/lint-rules/data/agent_identity_conformance.tsv"
_read_conformance "$_conformance" || exit 2

# The members of a pipe-delimited set, sorted, one to a line.
_members() {
    local s="${1#|}"
    s="${s%|}"
    printf '%s\n' "${s//|/$'\n'}" | sort -u
}

# What a mailbox glob stands for once its stars are filled in.
_instance() { printf '%s' "${1//\*/x}"; }

# Whether the first argument is one of the rest.
_is_in() {
    local needle="$1" x
    shift
    for x in "$@"; do
        [[ "$x" == "$needle" ]] && return 0
    done
    return 1
}
