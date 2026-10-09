#!/usr/bin/env bash
# =============================================================================
# mockspace/attribution/policy.sh - the caller's policy and the message scan
# =============================================================================
# Part of the attribution library, sourced by `lib/attribution.sh` and reached
# through `use mockspace::attribution` like the rest of it, not on its own.
# =============================================================================

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
