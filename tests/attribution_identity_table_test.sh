#!/usr/bin/env nutshell
# shellcheck shell=bash
# =============================================================================
# attribution_identity_table_test - the conformance table, and the rows in every form
# =============================================================================
# Run: ./test tests/attribution_identity_table_test.sh
#
# What names an agent is held to the conformance table that the lint pack's Rust
# suite reads too (`lint-rules/data/agent_identity_conformance.tsv`), so the two
# recognisers are held to one set of rows and neither can move alone. This suite
# reads the table and compares every list with the library's, then runs every
# person row and every agent row in each form a caller hands an identity in.
# =============================================================================

use test

# The library, the table and the helpers every identity suite shares.
source "$(dirname "${BASH_SOURCE[0]}")/identity_table.sh"

#[test]
it_reads_a_table_with_rows_in_every_list() {
    # The guard under all of it: a table that parsed to nothing would make every
    # loop below pass having asserted nothing.
    local n
    for n in T_marker T_mailbox T_tag T_tool T_given T_vendor T_machine T_head T_companion \
             T_keyed_name T_person T_agent; do
        declare -n _list="$n"
        assert_ok test "${#_list[@]}" -gt 0
    done
}

#[test]
it_refuses_a_table_line_that_is_not_a_row() {
    local f
    f="$(mktemp)"
    printf 'tool\n' > "$f";                 assert_fails _read_conformance "$f" 2>/dev/null
    printf 'tool\t\n' > "$f";               assert_fails _read_conformance "$f" 2>/dev/null
    printf 'nonsense\tx\n' > "$f";          assert_fails _read_conformance "$f" 2>/dev/null
    # a row that pairs two things needs both, and one that holds a single thing
    # takes no second
    printf 'vendor\tclaude\n' > "$f";       assert_fails _read_conformance "$f" 2>/dev/null
    printf 'vendor\tclaude\t\n' > "$f";     assert_fails _read_conformance "$f" 2>/dev/null
    printf 'keyed\tclaude\n' > "$f";        assert_fails _read_conformance "$f" 2>/dev/null
    printf 'tool\tclaude\tx\n' > "$f";      assert_fails _read_conformance "$f" 2>/dev/null
    printf 'tag\taider\tx\n' > "$f";        assert_fails _read_conformance "$f" 2>/dev/null
    # and the comment and blank lines that are allowed are not errors
    printf '# a note\n\ntool\tclaude\n' > "$f"
    assert_ok _read_conformance "$f"
    assert_eq "${T_tool[*]}" "claude"
    rm -f "$f"
}

#[test]
it_reads_a_pair_as_its_two_halves() {
    local f
    f="$(mktemp)"
    printf 'vendor\tclaude\tanthropic.com\nkeyed\tclaude\tclaude <root@buildhost.local>\n' > "$f"
    assert_ok _read_conformance "$f"
    assert_eq "${T_vendor[*]}" "claude anthropic.com"
    assert_eq "${T_keyed_name[*]}" "claude"
    assert_eq "${T_keyed_identity[*]}" "claude <root@buildhost.local>"
    rm -f "$f"
}

#[test]
it_holds_exactly_the_lists_the_table_holds() {
    assert_eq "$(_members "$ATTRIBUTION_AGENT_MARKERS")" "$(printf '%s\n' "${T_marker[@]}" | sort -u)"
    assert_eq "$(_members "$ATTRIBUTION_AGENT_MAILBOXES")" "$(printf '%s\n' "${T_mailbox[@]}" | sort -u)"
    assert_eq "$(_members "$ATTRIBUTION_AGENT_TAGS")" "$(printf '%s\n' "${T_tag[@]}" | sort -u)"
    assert_eq "$(_members "$ATTRIBUTION_AGENT_TOOLS")" "$(printf '%s\n' "${T_tool[@]}" | sort -u)"
    assert_eq "$(_members "$ATTRIBUTION_AGENT_GIVEN")" "$(printf '%s\n' "${T_given[@]}" | sort -u)"
    assert_eq "$(_members "$ATTRIBUTION_AGENT_VENDORS")" "$(printf '%s\n' "${T_vendor[@]}" | sort -u)"
    assert_eq "$(_members "$ATTRIBUTION_AGENT_MACHINES")" "$(printf '%s\n' "${T_machine[@]}" | sort -u)"
    assert_eq "$(_members "$ATTRIBUTION_AGENT_HEADS")" "$(printf '%s\n' "${T_head[@]}" | sort -u)"
    assert_eq "$(_members "$ATTRIBUTION_AGENT_COMPANIONS")" "$(printf '%s\n' "${T_companion[@]}" | sort -u)"
}

# --- the vendor net, across families ------------------------------------------

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
#[test]
it_judges_each_person_row_as_a_person_in_every_form_it_arrives_in() {
    local p
    for p in "${T_person[@]}"; do
        assert_fails attribution_names_agent "$p"
        assert_fails attribution_names_agent "Co-authored-by: $p"
        assert_fails attribution_names_agent "--author=\"$p\""
        assert_fails attribution_names_agent "$p 1791567502 +0000"
        # a name the caller passes that is not the person's changes nothing
        assert_fails attribution_names_agent "$p" 'Zed'
    done
}

#[test]
it_judges_each_agent_row_as_an_agent_in_every_form_it_arrives_in() {
    local a
    for a in "${T_agent[@]}"; do
        assert_ok attribution_names_agent "$a"
        assert_ok attribution_names_agent "Co-authored-by: $a"
        assert_ok attribution_names_agent "--author=\"$a\""
        assert_ok attribution_names_agent "$a 1791567502 +0000"
    done
}

#[test]
it_reads_the_forms_a_caller_hands_an_identity_in() {
    # A hook hands this the identity git would write, the identity a command
    # gives, and the halves of one given through the environment, one at a time.
    assert_ok attribution_names_agent 'Claude <noreply@anthropic.com>'
    assert_ok attribution_names_agent 'Claude <noreply@anthropic.com> 1791567502 +0000'
    assert_ok attribution_names_agent '--author="Claude <noreply@anthropic.com>"'
    assert_ok attribution_names_agent "--author='Claude <noreply@anthropic.com>'"
    assert_ok attribution_names_agent '--author Claude <noreply@anthropic.com>'
    assert_ok attribution_names_agent 'GIT_AUTHOR_NAME="Claude Code"'
    assert_ok attribution_names_agent 'GIT_COMMITTER_NAME=Copilot'
    assert_ok attribution_names_agent 'GIT_AUTHOR_EMAIL=noreply@anthropic.com'
    assert_ok attribution_names_agent 'noreply@anthropic.com'
    assert_ok attribution_names_agent 'Claude Code'
    assert_ok attribution_names_agent 'GIT_AUTHOR_NAME=Claude Agent SDK'
    assert_fails attribution_names_agent '--author="Jane Smith <jane@example.com>"'
    assert_fails attribution_names_agent 'GIT_AUTHOR_NAME="Claude Monet"'
    assert_fails attribution_names_agent 'GIT_AUTHOR_NAME=Claude'
    assert_fails attribution_names_agent 'GIT_AUTHOR_EMAIL=jane@example.com'
    assert_fails attribution_names_agent 'jane@example.com'
    assert_fails attribution_names_agent ''
    # a name the caller passes reaches an identity in each of those forms
    assert_ok attribution_names_agent '--author="Claude <root@buildhost.local>"' 'Claude'
    assert_ok attribution_names_agent 'GIT_AUTHOR_NAME=Claude' 'Claude'
    assert_ok attribution_names_agent 'Claude <root@buildhost.local> 1791567502 +0000' 'Claude'
}

#[test]
it_holds_no_command_substitution_in_the_functions_that_judge_an_identity() {
    # The claim in the header of the identity net, held. A hook calls this for
    # every commit in a push, and a command substitution is a subshell each. The
    # functions keep their words in a global array for that reason, so what is
    # checked is that none of them has grown a `$(...)` or a backtick. Arithmetic
    # expansion, `$((...))`, is no process and is let through.
    local fn body
    for fn in attribution_names_agent _attribution_words _attribution_words_are_a_tag \
              _attribution_words_are_a_named_agent _attribution_words_name_a_tool \
              _attribution_tools_at _attribution_tail_names_a_tool _attribution_is_a_version \
              _attribution_mailbox_is_the_tools; do
        assert_ok declare -F "$fn"
        body="$(declare -f "$fn")"
        assert_fails grep -qE '\$\([^(]|`' <<< "$body"
    done
}

#[test]
it_judges_a_trailer_by_its_key_and_the_advert_net_leaves_a_name_alone() {
    # The checks that were asked for after the matcher above was found refusing
    # people: do the other two nets have the same false positive on a
    # `Co-Authored-By` naming one? They do not, and these pin why. The trailer
    # net reads the key and never the name, so it neither refuses nor spares a
    # person by what they are called, and the advert net holds domains, mailboxes
    # and the robot emoji, none of which a name is.
    local who
    for who in 'Devin Smith' 'Hubbard Jones' 'Haider Ali' 'Cody Brown' 'Claude Monet' \
               'Anders Android' 'Jules Verne' 'Mistral Winds' 'Ada Lombard'; do
        assert_ok attribution_is_attribution_trailer "Co-authored-by: ${who} <p@example.com>"
        assert_fails attribution_has_advert "Co-authored-by: ${who} <p@example.com>"
    done
}
