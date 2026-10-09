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
# reaches it as `use mockspace::attribution`, with mockspace in its nut.toml and
# the commit pinned in nut.lock; from inside mockspace there is no dep to name
# and no namespace for the repository's own `lib/`.
#
# Anchored to this file rather than to NUTSHELL_SCRIPT_DIR, which names the
# entry point and moved out from under this the moment the runner became
# `./test` at the repository root.
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/lib/attribution.sh"

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
    ATTRIBUTION_AGENT_TOOLS=''
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

#[test]
it_reads_a_byline_behind_whitespace_or_a_run_of_hashes() {
    # The anchor was the hole, and one leading space was enough to get through
    # it. The `#` spellings matter more: measured on git 2.55.0, `git commit -F`
    # stores `# # Co-Authored-By: ...` in the body verbatim while git's own
    # trailer parser returns nothing for it, so a byline written that way passed
    # the parser the callers use for commit trailers and passed this net too.
    #
    # The fixtures are generated from the alphabet of spaces, tabs and hashes
    # rather than hand-picked, because the two previous repairs of this shape in
    # the commit-msg hook each named the spellings somebody had thought of and
    # the next spelling got through both.
    local b='Co-authored-by: Somebody <nobody@example.invalid>'
    local line
    for line in "$b" " $b" "  $b" "	$b" "#$b" "##$b" "###$b" "# $b" "#	$b" \
                "  # $b" "# # $b" "# #$b" "	#  #	$b" "### # ###$b" " # # # $b"; do
        assert_ok attribution_is_attribution_trailer "$line"
    done
}

#[test]
it_does_not_read_a_byline_behind_a_quote_or_a_list_marker() {
    # Whitespace and `#` is the whole of what comes off, and it is not the whole of
    # what defeats an anchor. Each prefix here hides a byline from this net, and
    # measured on git 2.55.0 `git commit -F` stores all of them in a commit body
    # verbatim while git's own trailer parser reports none.
    #
    # The arm asserts the boundary rather than the repair, because the repair belongs
    # to the caller. This net reads markdown as well as commit messages, where `>`
    # opens a quotation and `-` opens a list, so refusing those here would refuse
    # ordinary prose in a readme, and a false refusal costs a rehoused repository. A
    # caller whose surface is only a commit message can take them off before it calls;
    # nothing here does it for one, and the earlier version of this comment said a
    # consumer already did, which was a claim about somebody else's tree and was wrong
    # about the one it named.
    local b='Co-authored-by: Somebody <nobody@example.invalid>'
    local line
    for line in "> $b" ">> $b" "// $b" "/* $b" "- $b" "| $b" "* $b" ".$b" "> - $b"; do
        assert_fails attribution_is_attribution_trailer "$line"
    done
}

#[test]
it_reads_an_indented_author_line_as_a_trailer() {
    # What the strip costs, pinned so that nobody has to rediscover it. `author` and
    # `committer` sit in the key pattern as bare words, a git author field having no
    # `-by` shape, so with the anchor gone an indented one reads as a trailer: a
    # markdown code block indented by four spaces, a YAML document, a struct literal
    # in a diff. `attribution_strip_quoted` knows a fence and an inline backtick and
    # not an indented block, and the trailer path does not call it anyway.
    #
    # The unindented spelling matched before this net was widened, so what is new is
    # the indentation and nothing else, which is why the arm carries both. Measured
    # over both repositories' whole histories when it landed, the widening matched no
    # line that was not already matched, so this is a shape rather than a report.
    #
    # It is a pin on today's answer. An arm saying a thing happens is not an
    # argument that it should, and the day somebody narrows those two keys this goes
    # red and is the place to argue it.
    assert_ok attribution_is_attribution_trailer '    author: Jane Roe'
    assert_ok attribution_is_attribution_trailer 'author: Jane Roe'
    assert_ok attribution_is_attribution_trailer '    committer: Jane Roe'
    # And the neighbours that stay out, so the arm says where the edge is rather than
    # only that there is one. A plural key is not the key, and an assignment is not a
    # colon.
    assert_fails attribution_is_attribution_trailer '    Authors: Jane and Bob'
    assert_fails attribution_is_attribution_trailer '    author = "Jane"'
}

#[test]
it_does_not_read_a_quoted_or_headed_mention_as_a_trailer() {
    # The other half of the trade. A quotation is safe because the convention
    # here is to backtick the forbidden string, and a leading backtick is not
    # stripped; a heading that merely names the key carries no value after a
    # colon, so it fails on the same requirement prose has always failed on.
    assert_fails attribution_is_attribution_trailer \
        '`Co-authored-by: Somebody <nobody@example.invalid>`'
    assert_fails attribution_is_attribution_trailer \
        '# what a co-authored-by line is for'
    assert_fails attribution_is_attribution_trailer '## Co-authored-by'
    assert_fails attribution_is_attribution_trailer '# # Co-authored-by:'
    assert_fails attribution_is_attribution_trailer '####'
    assert_fails attribution_is_attribution_trailer '   '
}

#[test]
it_reports_a_byline_a_message_hid_behind_a_hash() {
    # The surface claim rather than the predicate: a whole message carrying the
    # commented spelling is a finding, which is what a caller scanning a tag or
    # a pull request body actually asks.
    #
    # The whole record, both lines of it, rather than the word `trailer` somewhere
    # in the output. A scan naming the wrong line, or naming three, satisfies
    # `assert_contains` just as well, and what a caller prints to somebody is the
    # excerpt rather than the kind. The advert line is here because the address in
    # the fixture is one, which is what a real byline of this shape carries.
    local msg out
    msg='feat: a thing

A body that explains the thing.

# # Co-authored-by: Claude <noreply@anthropic.com>'
    out="$(attribution_scan_message "$msg" '')"
    assert_eq "$out" \
"trailer	# # Co-authored-by: Claude <noreply@anthropic.com>
advert	# # Co-authored-by: Claude <noreply@anthropic.com>"
}

#[test]
it_reports_a_byline_that_is_not_in_the_final_block() {
    # The header over `attribution_scan_message` claimed for a long time that the
    # trailer net read the message's final block, and the code has never done
    # that. Which behaviour is right is not a matter of taste here: a byline
    # somebody hid in the middle of a body is exactly what a scan of a tag or a
    # pull request body is asked about, and a final-block reader answers clean on
    # it. So the arm pins the whole-text scan, and the header now says so.
    local msg out
    msg='feat: a thing

Co-authored-by: Claude <noreply@anthropic.com>

A body that goes on afterwards, so the byline is nowhere near the end.

Closes #1.'
    out="$(attribution_scan_message "$msg" '')"
    assert_eq "$out" \
"trailer	Co-authored-by: Claude <noreply@anthropic.com>
advert	Co-authored-by: Claude <noreply@anthropic.com>"
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

# What names an agent is what only an agent carries: a mailbox an agent commits
# from, a bot marker, or a name that is wholly a tool's own, behind a vendor word
# and followed by the words that ride along with one. It is never a word inside
# somebody's name. The lists and the verdicts are the conformance table that the
# lint pack's Rust suite reads too (`lint-rules/data/agent_identity_conformance.tsv`),
# so the two recognisers are held to one set of rows and neither can move alone.
#
# The matrix below is built from the table, and it is the table that is the
# claim: a test that walked the library's own lists would pass with an entry
# deleted from them, so every list is also compared with the table's, and every
# entry is exercised on its own.

_conformance="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/lint-rules/data/agent_identity_conformance.tsv"
T_marker=() T_mailbox=() T_tool=() T_given=() T_head=() T_companion=() T_person=() T_agent=()
while IFS=$'\t' read -r _kind _value; do
    [[ -z "$_kind" || "$_kind" == \#* ]] && continue
    case "$_kind" in
        marker)    T_marker+=("$_value") ;;
        mailbox)   T_mailbox+=("$_value") ;;
        tool)      T_tool+=("$_value") ;;
        given)     T_given+=("$_value") ;;
        head)      T_head+=("$_value") ;;
        companion) T_companion+=("$_value") ;;
        person)    T_person+=("$_value") ;;
        agent)     T_agent+=("$_value") ;;
        *) printf 'attribution_test: unknown kind in the conformance table: %s\n' "$_kind" >&2; exit 2 ;;
    esac
done < "$_conformance"
unset _kind _value

# The members of a pipe-delimited set, sorted, one to a line.
_members() {
    local s="${1#|}"
    s="${s%|}"
    printf '%s\n' "${s//|/$'\n'}" | sort -u
}

# What a mailbox glob stands for once its stars are filled in.
_instance() { printf '%s' "${1//\*/x}"; }

#[test]
it_reads_a_table_with_rows_in_every_list() {
    # The guard under all of it: a table that parsed to nothing would make every
    # loop below pass having asserted nothing.
    local n
    for n in T_marker T_mailbox T_tool T_given T_head T_companion T_person T_agent; do
        declare -n _list="$n"
        assert_ok test "${#_list[@]}" -gt 0
    done
}

#[test]
it_holds_exactly_the_lists_the_table_holds() {
    assert_eq "$(_members "$ATTRIBUTION_AGENT_MARKERS")" "$(printf '%s\n' "${T_marker[@]}" | sort -u)"
    assert_eq "$(_members "$ATTRIBUTION_AGENT_MAILBOXES")" "$(printf '%s\n' "${T_mailbox[@]}" | sort -u)"
    assert_eq "$(_members "$ATTRIBUTION_AGENT_TOOLS")" "$(printf '%s\n' "${T_tool[@]}" | sort -u)"
    assert_eq "$(_members "$ATTRIBUTION_AGENT_GIVEN")" "$(printf '%s\n' "${T_given[@]}" | sort -u)"
    assert_eq "$(_members "$ATTRIBUTION_AGENT_HEADS")" "$(printf '%s\n' "${T_head[@]}" | sort -u)"
    assert_eq "$(_members "$ATTRIBUTION_AGENT_COMPANIONS")" "$(printf '%s\n' "${T_companion[@]}" | sort -u)"
}

#[test]
it_names_each_tool_as_the_whole_of_a_name_and_nothing_longer() {
    local t
    for t in "${T_tool[@]}"; do
        assert_ok attribution_names_agent "$t <x@example.com>"
        assert_ok attribution_names_agent "${t^^}"
        # a word of somebody's name on either side is a person
        assert_fails attribution_names_agent "$t Smith <x@example.com>"
        assert_fails attribution_names_agent "Smith $t <x@example.com>"
    done
}

#[test]
it_asks_a_second_signal_of_each_tool_that_is_also_a_given_name() {
    local g c h m
    for g in "${T_given[@]}"; do
        assert_fails attribution_names_agent "$g <x@example.com>"
        assert_fails attribution_names_agent "$g"
        # a version, a companion after it, a vendor word before it, a mailbox
        assert_ok attribution_names_agent "$g 4.1 <x@example.com>"
        assert_ok attribution_names_agent "$g v2"
        for c in "${T_companion[@]}"; do
            assert_ok attribution_names_agent "$g $c <x@example.com>"
        done
        for h in "${T_head[@]}"; do
            assert_ok attribution_names_agent "$h $g <x@example.com>"
        done
        for m in "${T_mailbox[@]}"; do
            assert_ok attribution_names_agent "$g <$(_instance "$m")>"
        done
        # and a word that is none of those is a person
        assert_fails attribution_names_agent "$g Monet <x@example.com>"
        assert_fails attribution_names_agent "Max $g <x@example.com>"
    done
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

# Whether the first argument is one of the rest.
_is_in() {
    local needle="$1" x
    shift
    for x in "$@"; do
        [[ "$x" == "$needle" ]] && return 0
    done
    return 1
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

#[test]
it_judges_each_person_row_as_a_person_in_every_form_it_arrives_in() {
    local p name
    for p in "${T_person[@]}"; do
        assert_fails attribution_names_agent "$p"
        assert_fails attribution_names_agent "Co-authored-by: $p"
        assert_fails attribution_names_agent "--author=\"$p\""
        assert_fails attribution_names_agent "$p 1791567502 +0000"
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
    assert_fails attribution_names_agent '--author="Jane Smith <jane@example.com>"'
    assert_fails attribution_names_agent 'GIT_AUTHOR_NAME="Claude Monet"'
    assert_fails attribution_names_agent 'GIT_AUTHOR_NAME=Claude'
    assert_fails attribution_names_agent 'GIT_AUTHOR_EMAIL=jane@example.com'
    assert_fails attribution_names_agent 'jane@example.com'
    assert_fails attribution_names_agent ''
}

#[test]
it_holds_no_command_substitution_in_the_functions_that_judge_an_identity() {
    # The claim in the header of the identity net, held. A hook calls this for
    # every commit in a push, and a command substitution is a subshell each. The
    # functions keep their words in a global array for that reason, so what is
    # checked is that none of them has grown a `$(...)` or a backtick. Arithmetic
    # expansion, `$((...))`, is no process and is let through.
    local fn body
    for fn in attribution_names_agent _attribution_words _attribution_words_name_a_tool \
              _attribution_tools_at _attribution_tail_is_companions; do
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

#[test]
it_does_not_condemn_a_quotation_that_wraps() {
    # Prose wraps, and a commit body wraps hardest, so the quotation the row
    # above pins arrives split across two lines as often as not. The strip was
    # line-oriented, so the closing backtick sat on a line the opening one was
    # not on and the span was never a span.
    #
    # What sent me looking was two commits on arvo's trunk that the scan
    # refuses, and they turned out not to be this case: their body carries one
    # backtick and no opening one, so the URL in them is not inside a span at
    # all and the scan is right about them. This row is the case I expected to
    # find there and did not, which is worth pinning on its own: a rule file
    # quoting the suffix in a wrapped line is ordinary, and nothing was
    # guarding it.
    local wrapped
    wrapped='Adds a new "Forbidden suffix" section that removes `🤖 Generated with [Claude
Code](https://claude.com/claude-code)` (and any equivalent
"generated by AI tool X" advertising line) from commit message bodies.'
    assert_fails attribution_has_advert "$wrapped"
}

#[test]
it_does_not_condemn_innocent_prose_that_wraps_before_a_link() {
    # The widening the repair must not buy. A paragraph read as one record
    # still has to keep its line breaks, or the phrase net reads "generated
    # by" on one line together with the link that starts the next, and a
    # README saying which tool generated some bindings, or a body signing off
    # with an address, is condemned. The comment on the strip says a false
    # positive here triggers an irreversible repair, which is why both shapes
    # are pinned.
    local prose
    prose='The bindings in this crate are generated by
[the codegen tool](https://example.com/codegen), so do not edit them by hand.'
    assert_fails attribution_has_advert "$prose"
    local signed
    signed='This file is written by
<ort@hiisi.digital> and nobody else.'
    assert_fails attribution_has_advert "$signed"
    # and the control: the same phrase on one line is what the net is for
    assert_ok attribution_has_advert 'generated by [tool](https://example.com/x)'
}

#[test]
it_still_catches_an_advert_a_stray_backtick_sits_near() {
    # The bound on the repair, and the reason it is paragraph-shaped rather than
    # a slurp of the whole message. Joining every line lets one unbalanced
    # backtick swallow everything up to the next one anywhere later, which would
    # hide a real suffix behind a stray quote somebody wrote paragraphs above.
    #
    # A blank line closes the span whatever the backticks are doing, so the
    # suffix in its own paragraph is still read. That is where a tool puts it.
    local stray
    stray='A body with an unclosed `span in it, and prose after it that runs on
for a line or two the way a real body does.

🤖 Generated with [Claude Code](https://claude.com/claude-code)'
    assert_ok attribution_has_advert "$stray"
}

#[test]
it_still_catches_an_advert_wrapped_across_two_lines() {
    # The other direction. A suffix that wraps is still a suffix: nothing about
    # the repair depends on a match sitting on one line, only on the backticks
    # around it, and there are none here.
    local wrapped
    wrapped='chore: a thing

🤖 Generated with [Claude
Code](https://claude.com/claude-code)'
    assert_ok attribution_has_advert "$wrapped"
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
