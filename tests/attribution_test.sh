#!/usr/bin/env nutshell
# shellcheck shell=bash
# =============================================================================
# attribution_test - every pattern against what it must and must not match
# =============================================================================
# Run: nutshell -c 'use test; test_run tests/attribution_test.sh; test_summary'
#
# Every "must not" case below is a false positive somebody actually hit, or one
# the engine was widened past. Deleting a row frees the pattern it guards to
# swallow prose again, which is how the predecessor scanner came to report a
# repository as contaminated over the word "written".
#
# The rows that matter most are the ones with no vendor in them. The shape net
# is what catches a tool nobody has heard of yet, so a test suite that only
# checks the forty known vendors is testing the net that is allowed to go stale.
# =============================================================================

use test

# Sourced by path rather than reached with `use`, because `use <name>` searches
# nutshell's own library set and this one is mockspace's. A consumer repository
# reaches it as `use mockspace::attribution` with mockspace in its nut.toml;
# from inside mockspace there is no dep to name.
source "${NUTSHELL_SCRIPT_DIR}/../lib/attribution.sh"

# The guard that has to come before every assertion below.
#
# A missing function exits 127, and 127 is a non-zero exit, so `assert_fails`
# reads "the command does not exist" as "the predicate correctly said no". Nine
# of these tests passed that way while the library was not loading at all. So
# existence is asserted once, explicitly, rather than inferred from behaviour.
for _fn in attribution_selfcheck attribution_is_attribution_trailer \
           attribution_names_agent attribution_has_advert \
           attribution_advert_excerpt attribution_allows \
           attribution_scan_message attribution_strip_quoted; do
    if ! declare -F "$_fn" >/dev/null; then
        printf 'attribution_test: %s is not defined; the library did not load\n' "$_fn" >&2
        printf 'every assert_fails below would pass on exit 127 and mean nothing\n' >&2
        exit 2
    fi
done
unset _fn

# --- the engine's own assertion ----------------------------------------------

#[test]
it_selfchecks_before_anything_else_trusts_it() {
    assert_ok attribution_selfcheck
}

#[test]
it_fails_its_selfcheck_when_a_pattern_is_emptied() {
    # The control for the control. A selfcheck that cannot fail is decoration,
    # and this engine exists partly because a check that silently passed was
    # indistinguishable from a check that ran.
    ATTRIBUTION_IDENTITY_RE=''
    assert_fails attribution_selfcheck
}

#[test]
it_fails_its_selfcheck_when_the_shape_net_matches_everything() {
    # A net that matches a commit subject never reports clean either, which is
    # the same defect pointed the other way.
    ATTRIBUTION_TRAILER_KEY_RE='[A-Za-z-]+'
    assert_fails attribution_selfcheck
}

# --- the shape net, which is the one that keeps working ----------------------

#[test]
it_recognises_an_attribution_by_shape_without_knowing_the_vendor() {
    # The whole argument for the shape net: a tool that does not exist yet.
    assert_ok attribution_is_attribution_trailer \
        'Co-authored-by: Hypothetical Agent 9 <nobody@example.invalid>'
    assert_ok attribution_is_attribution_trailer \
        'Assisted-by: Something Nobody Has Heard Of <x@y.z>'
    assert_ok attribution_is_attribution_trailer 'Generated-With: some-tool'
    assert_ok attribution_is_attribution_trailer 'Agent-Session: 01ABC'
}

#[test]
it_does_not_read_a_conventional_commit_subject_as_a_trailer() {
    # `docs: ...` is `Key: value` shaped and mentions a vendor. Matching it
    # would condemn a repository for a commit about a feature.
    assert_fails attribution_is_attribution_trailer \
        'docs: describe how the copilot integration surface is configured'
    assert_fails attribution_is_attribution_trailer \
        'refactor: rename the codex module to catalogue'
    assert_fails attribution_is_attribution_trailer 'fix: a bug'
    assert_fails attribution_is_attribution_trailer 'Fixes: #1234'
}

#[test]
it_requires_a_value_after_the_colon() {
    assert_fails attribution_is_attribution_trailer 'Co-authored-by:'
    assert_fails attribution_is_attribution_trailer 'Co-authored-by:   '
}

# --- policy is the caller's, and absent policy refuses -----------------------

#[test]
it_permits_nothing_when_the_caller_states_no_policy() {
    # A missing config fails toward refusing a byline rather than allowing one,
    # which is the only safe direction: the cost of a wrong refusal is a
    # conversation, the cost of a wrong allowance is permanent.
    assert_fails attribution_allows 'Co-authored-by: A Human <a@b.c>' ''
}

#[test]
it_permits_exactly_what_the_caller_names() {
    assert_ok attribution_allows 'Co-authored-by: A Human <a@b.c>' '*<a@b.c>'
    assert_fails attribution_allows 'Co-authored-by: Claude <x@y.z>' '*<a@b.c>'
}

# --- the vendor net, for surfaces with no shape ------------------------------

#[test]
it_names_the_vendors_across_families() {
    local line
    for line in \
        'Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>' \
        'Co-authored-by: Copilot <copilot@github.com>' \
        'Co-authored-by: ChatGPT <noreply@openai.com>' \
        'Co-authored-by: Cursor Agent <agent@cursor.com>' \
        'Co-authored-by: google-labs-jules[bot] <jules@google.com>' \
        'Co-authored-by: Devin AI <devin@cognition-labs.com>' \
        'Co-authored-by: aider <aider@aider.chat>' \
        'Co-authored-by: Amp <amp@ampcode.com>' \
        'Co-authored-by: Grok <grok@x.ai>' \
        'Co-authored-by: dependabot[bot] <support@github.com>'
    do
        assert_ok attribution_names_agent "$line"
    done
}

#[test]
it_does_not_name_a_human() {
    assert_fails attribution_names_agent 'Co-authored-by: Jane Doe <jane@example.com>'
    assert_fails attribution_names_agent 'Reviewed-by: a human being <human@example.com>'
}

# --- adverts, and the prose they must not eat --------------------------------

#[test]
it_finds_the_advert_suffixes_tools_append() {
    assert_ok attribution_has_advert '🤖 Generated with [Claude Code](https://claude.com/claude-code)'
    assert_ok attribution_has_advert 'Generated with [Claude Code](https://claude.ai/code)'
    assert_ok attribution_has_advert 'Created with [Cursor](https://cursor.com)'
    assert_ok attribution_has_advert 'Written by [Copilot](https://github.com/features/copilot)'
    assert_ok attribution_has_advert 'Built with <https://devin.ai>'
    assert_ok attribution_has_advert 'a line with a 🤖 in it'
}

#[test]
it_does_not_eat_prose_that_merely_uses_the_words() {
    # The first row is the real commit subject that made a predecessor report
    # this repository as contaminated.
    assert_fails attribution_has_advert \
        'fix: accept a placeholder written with spaces inside its braces'
    assert_fails attribution_has_advert \
        'docs: the section was written with care and is the better for it'
    assert_fails attribution_has_advert \
        'feat: the report is generated by the renderer rather than by hand'
    assert_fails attribution_has_advert 'chore: bump the generated bindings'
}

#[test]
it_does_not_condemn_a_document_for_quoting_the_rule() {
    # The rule files in this workspace quote these strings in order to forbid
    # them. A scanner that cannot tell a rule from a violation condemns the rule,
    # and a predecessor did exactly that to its own documentation.
    assert_fails attribution_has_advert \
        'The suffix `🤖 Generated with [Claude Code](https://claude.com/claude-code)` is forbidden.'
}

# --- the whole-message scan ---------------------------------------------------

#[test]
it_reports_the_trailer_and_the_advert_from_one_message() {
    local msg out
    msg='feat: a thing

A body that explains the thing.

Co-authored-by: Claude <noreply@anthropic.com>
🤖 Generated with [Claude Code](https://claude.com/claude-code)'
    out="$(attribution_scan_message "$msg" '')"
    assert_contains "$out" 'trailer'
    assert_contains "$out" 'advert'
}

#[test]
it_reports_nothing_from_a_clean_message() {
    local out
    out="$(attribution_scan_message 'feat: a thing

A body written with care, mentioning claude code in passing.' '')"
    assert_empty "$out"
}

#[test]
it_reports_a_trailer_the_policy_does_not_cover_and_not_one_it_does() {
    local msg out
    msg='feat: a thing

Co-authored-by: A Human <human@example.com>'
    assert_empty "$(attribution_scan_message "$msg" '*<human@example.com>')"
    out="$(attribution_scan_message "$msg" '*<someone-else@example.com>')"
    assert_contains "$out" 'trailer'
}
