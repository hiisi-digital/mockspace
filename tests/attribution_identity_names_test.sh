#!/usr/bin/env nutshell
# shellcheck shell=bash
# =============================================================================
# attribution_identity_names_test - what in a name makes an agent
# =============================================================================
# Run: ./test tests/attribution_identity_names_test.sh
#
# A tool is an agent as the start of a name, alone or followed by nothing but
# companions and versions; a tool that is also a given name wants a second signal
# when nothing follows it; a tag in parentheses is read and nothing else in them
# is. Every kind of row has an arm, and a row taken out of the table or an entry
# taken out of the library turns one red.
# =============================================================================

use test

# The library, the table and the helpers every identity suite shares.
source "$(dirname "${BASH_SOURCE[0]}")/identity_table.sh"

# --- one arm for every kind of row --------------------------------------------

#[test]
it_names_each_tool_as_the_start_of_a_name_and_nothing_inside_one() {
    local t c
    for t in "${T_tool[@]}"; do
        # alone, and in capitals, and behind a mailbox of nobody's
        assert_ok attribution_names_agent "$t <x@example.com>"
        assert_ok attribution_names_agent "${t^^}"
        # followed by nothing but companions and versions, a surface or a model
        assert_ok attribution_names_agent "$t Chat <x@example.com>"
        assert_ok attribution_names_agent "$t 4.1"
        assert_ok attribution_names_agent "$t v2 Chat"
        # followed by a word of somebody's name, with or without a comma, it is theirs
        assert_fails attribution_names_agent "$t Smith <x@example.com>"
        assert_fails attribution_names_agent "$t, Smith"
        # and one companion among such words does not turn them into the tool's
        assert_fails attribution_names_agent "$t Chat Smith"
        assert_fails attribution_names_agent "$t Smith Chat"
        assert_fails attribution_names_agent "$t Zed Chat Smith"
        # a word of somebody's name in front of it makes it a name inside theirs
        assert_fails attribution_names_agent "Smith $t <x@example.com>"
    done
    for c in "${T_companion[@]}"; do
        assert_ok attribution_names_agent "Copilot $c"
        assert_fails attribution_names_agent "Copilot Smith $c"
    done
}

#[test]
it_asks_a_second_signal_of_each_tool_that_is_also_a_given_name() {
    local g c h m
    for g in "${T_given[@]}"; do
        # alone it is a person's name
        assert_fails attribution_names_agent "$g <x@example.com>"
        assert_fails attribution_names_agent "$g"
        # with a vendor word before it, or a mailbox on the list, it is the tool
        for h in "${T_head[@]}"; do
            assert_ok attribution_names_agent "$h $g <x@example.com>"
        done
        for m in "${T_mailbox[@]}"; do
            assert_ok attribution_names_agent "$g <$(_instance "$m")>"
        done
        # followed by nothing but companions and versions it is the tool, as any tool is
        assert_ok attribution_names_agent "$g 4.1 <x@example.com>"
        assert_ok attribution_names_agent "$g v2"
        assert_ok attribution_names_agent "$g 4.1 ${T_companion[0]}"
        for c in "${T_companion[@]}"; do
            assert_ok attribution_names_agent "$g $c <x@example.com>"
            assert_ok attribution_names_agent "$g $c 4.1"
            # a word of somebody's name anywhere among them makes it a person's
            assert_fails attribution_names_agent "$g Zed $c <x@example.com>"
            assert_fails attribution_names_agent "$g $c Zed <x@example.com>"
            assert_fails attribution_names_agent "$g $c Zed $c"
        done
        # and a word that is none of those is a person's, a version included
        assert_fails attribution_names_agent "$g Monet <x@example.com>"
        assert_fails attribution_names_agent "$g 4.1 Monet <x@example.com>"
        assert_fails attribution_names_agent "$g Monet 4.1"
        assert_fails attribution_names_agent "Max $g <x@example.com>"
    done
}

#[test]
it_reads_a_group_in_parentheses_for_a_tag_and_for_nothing_else() {
    local tag t
    for tag in "${T_tag[@]}"; do
        assert_ok attribution_names_agent "Jane Doe ($tag)"
        assert_ok attribution_names_agent "Jane Doe (${tag^^}) <jane@example.com>"
        assert_ok attribution_names_agent "Jane (she/her) ($tag) Doe"
        # exactly the tag, so a group holding more is left out of the name
        assert_fails attribution_names_agent "Jane Doe ($tag extra)"
        assert_fails attribution_names_agent "Jane Doe ($tag-ish)"
    done
    # a tool's name, a given name or a vendor in a group is none of the tags
    for t in "${T_tool[@]}" "${T_given[@]}" "${T_head[@]}"; do
        _is_in "$t" "${T_tag[@]}" && continue
        assert_fails attribution_names_agent "Jane Doe ($t)"
        assert_fails attribution_names_agent "Jane Doe (${t^^}) <jane@example.com>"
    done
    # and a group that is left out of a name leaves the rest of it to be read
    assert_ok attribution_names_agent 'Copilot (she/her)'
    assert_ok attribution_names_agent 'Claude 3.5 Sonnet (new)'
    assert_fails attribution_names_agent 'Claude Monet (aider-ish)'
}

#[test]
it_counts_each_companion_after_a_tool_and_never_before_one() {
    local t c
    for t in "${T_tool[@]}" "${T_given[@]}"; do
        for c in "${T_companion[@]}"; do
            assert_ok attribution_names_agent "$t $c <x@example.com>"
            # in front of the tool it is a word of somebody's name, unless it is
            # a vendor word, or is itself a tool and so a name of its own
            if _is_in "$c" "${T_head[@]}" "${T_tool[@]}" "${T_given[@]}"; then
                continue
            fi
            assert_fails attribution_names_agent "$c $t <x@example.com>"
        done
    done
}

#[test]
it_lets_each_head_word_stand_before_a_tool() {
    local h t
    for h in "${T_head[@]}"; do
        for t in "${T_tool[@]}" "${T_given[@]}"; do
            assert_ok attribution_names_agent "$h $t <x@example.com>"
        done
        # a head word is not a tool, so on its own it names nobody
        if ! _is_in "$h" "${T_tool[@]}"; then
            assert_fails attribution_names_agent "$h <x@example.com>"
        fi
    done
}

#[test]
it_matches_each_mailbox_whole_whatever_the_name() {
    local m
    for m in "${T_mailbox[@]}"; do
        assert_ok attribution_names_agent "Dev Container <$(_instance "$m")>"
        assert_ok attribution_names_agent "$(_instance "$m")"
        # an exact address is not matched by a longer one that ends with it
        if [[ "$m" != *\** ]]; then
            assert_fails attribution_names_agent "Dev Container <x$m>"
            assert_fails attribution_names_agent "Dev Container <${m}.example>"
        fi
    done
}

#[test]
it_matches_each_marker_in_the_name_or_in_the_mailbox() {
    local m
    for m in "${T_marker[@]}"; do
        assert_ok attribution_names_agent "thing${m} <x@example.com>"
        assert_ok attribution_names_agent "A Name <1234+thing${m}@users.noreply.github.com>"
    done
}

