#!/usr/bin/env nutshell
# shellcheck shell=bash
# =============================================================================
# attribution_identity_test - what names an agent, held to the conformance table
# =============================================================================
# Run: ./test tests/attribution_identity_test.sh
#
# What names an agent is what only an agent carries: a bot marker, a mailbox an
# agent commits from, a tag a tool writes into a name, a name the caller passes,
# or a name that starts with a tool's own. It is never a word inside somebody's
# name. The lists and the verdicts are the conformance table that the lint
# pack's Rust suite reads too (`lint-rules/data/agent_identity_conformance.tsv`),
# so the two recognisers are held to one set of rows and neither can move alone.
#
# The matrix below is built from the table, and it is the table that is the
# claim: a test that walked the library's own lists would pass with an entry
# deleted from them, so every list is also compared with the table's, and every
# entry is exercised on its own. Every kind of row has an arm here, and a row
# taken out of the table or an entry taken out of the library turns one red.
# =============================================================================

use test

# Sourced by path for the reason `attribution_test.sh` gives.
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/lib/attribution.sh"

# A missing function exits 127, and `assert_fails` reads that as a correct
# refusal, so existence is asserted once, here, and not inferred from behaviour.
for _fn in attribution_names_agent attribution_is_attribution_trailer attribution_has_advert \
           _attribution_words _attribution_words_are_a_tag _attribution_words_are_a_named_agent \
           _attribution_words_name_a_tool _attribution_tools_at _attribution_tail_names_a_tool \
           _attribution_is_a_version _attribution_mailbox_is_the_tools; do
    if ! declare -F "$_fn" >/dev/null; then
        printf 'attribution_identity_test: %s is not defined; the library did not load\n' "$_fn" >&2
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
    T_marker=() T_mailbox=() T_tag=() T_tool=() T_given=() T_vendor=() T_head=()
    T_companion=() T_keyed_name=() T_keyed_identity=() T_person=() T_agent=()
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

#[test]
it_reads_a_table_with_rows_in_every_list() {
    # The guard under all of it: a table that parsed to nothing would make every
    # loop below pass having asserted nothing.
    local n
    for n in T_marker T_mailbox T_tag T_tool T_given T_vendor T_head T_companion \
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

# --- one arm for every kind of row --------------------------------------------

#[test]
it_names_each_tool_as_the_start_of_a_name_and_nothing_inside_one() {
    local t
    for t in "${T_tool[@]}"; do
        assert_ok attribution_names_agent "$t <x@example.com>"
        assert_ok attribution_names_agent "${t^^}"
        # any words after it, a surface or a product or somebody's name
        assert_ok attribution_names_agent "$t Chat <x@example.com>"
        assert_ok attribution_names_agent "$t Smith <x@example.com>"
        # a word of somebody's name in front of it makes it a name inside theirs
        assert_fails attribution_names_agent "Smith $t <x@example.com>"
    done
}

#[test]
it_asks_a_second_signal_of_each_tool_that_is_also_a_given_name() {
    local g c h m
    for g in "${T_given[@]}"; do
        assert_fails attribution_names_agent "$g <x@example.com>"
        assert_fails attribution_names_agent "$g"
        # a version after it, a companion anywhere after it, a vendor word before
        # it, a mailbox on the list
        assert_ok attribution_names_agent "$g 4.1 <x@example.com>"
        assert_ok attribution_names_agent "$g v2"
        for c in "${T_companion[@]}"; do
            assert_ok attribution_names_agent "$g $c <x@example.com>"
            # one companion among other words is enough
            assert_ok attribution_names_agent "$g Zed $c <x@example.com>"
            assert_ok attribution_names_agent "$g $c Zed <x@example.com>"
        done
        for h in "${T_head[@]}"; do
            assert_ok attribution_names_agent "$h $g <x@example.com>"
        done
        for m in "${T_mailbox[@]}"; do
            assert_ok attribution_names_agent "$g <$(_instance "$m")>"
        done
        # and a word that is none of those is a person's, and a version does not
        # change that
        assert_fails attribution_names_agent "$g Monet <x@example.com>"
        assert_fails attribution_names_agent "$g 4.1 Monet <x@example.com>"
        assert_fails attribution_names_agent "Max $g <x@example.com>"
    done
}

#[test]
it_reads_a_given_name_behind_a_mailbox_that_is_the_tools() {
    local g v tool domain d
    for g in "${T_given[@]}"; do
        # the local part is the tool's word, whole, at a mailbox that is a
        # machine's: localhost, a domain of one label, a name only a private
        # network uses, or GitHub's noreply form with the login after the number
        assert_ok attribution_names_agent "$g <$g@localhost>"
        assert_ok attribution_names_agent "$g <$g@buildbox>"
        assert_ok attribution_names_agent "${g^^} <${g^^}@BUILDBOX>"
        for d in local localdomain lan internal home.arpa; do
            assert_ok attribution_names_agent "$g <$g@ci.$d>"
            assert_ok attribution_names_agent "${g^^} <${g^^}@CI.${d^^}>"
        done
        assert_ok attribution_names_agent "$g <12345+$g@users.noreply.github.com>"
        assert_ok attribution_names_agent "${g^^} <12345+${g^^}@USERS.NOREPLY.GITHUB.COM>"
        # the same local part at an ordinary domain is a person called that
        assert_fails attribution_names_agent "$g <$g@example.com>"
        assert_fails attribution_names_agent "$g <$g@acme.com>"
        assert_fails attribution_names_agent "$g <$g@gmail.com>"
        assert_fails attribution_names_agent "${g^^} <${g^^}@EXAMPLE.COM>"
        # and so is the login without its number, and the number off GitHub
        assert_fails attribution_names_agent "$g <$g@users.noreply.github.com>"
        assert_fails attribution_names_agent "$g <12345+$g@example.com>"
        assert_fails attribution_names_agent "$g <12345+$g@ci.local>"
        # a name only a private network uses has to end the domain
        assert_fails attribution_names_agent "$g <$g@ci.local.example.com>"
        assert_fails attribution_names_agent "$g <$g@local.example.com>"
        assert_fails attribution_names_agent "$g <$g@lan.example.com>"
        assert_fails attribution_names_agent "$g <$g@x.home.arpa.example>"
        # the local part is the tool's word whole
        assert_fails attribution_names_agent "$g <x$g@localhost>"
        assert_fails attribution_names_agent "$g <$g.x@localhost>"
        assert_fails attribution_names_agent "$g <x$g@ci.local>"
        assert_fails attribution_names_agent "$g <12345+x$g@users.noreply.github.com>"
        # the mailbox is the whole of the signal, so it needs the whole of the name
        assert_fails attribution_names_agent "$g Monet <$g@localhost>"
        assert_fails attribution_names_agent "$g Monet <$g@ci.local>"
        # a bare mailbox has no name, a name with no mailbox has no domain, and a
        # mailbox with no domain is nobody's
        assert_fails attribution_names_agent "$g@localhost"
        assert_fails attribution_names_agent "$g"
        assert_fails attribution_names_agent "$g <$g@>"
    done
    # a vendor's domain, matched whole, and only the vendor of that tool
    for v in "${T_vendor[@]}"; do
        tool="${v%% *}"
        domain="${v#* }"
        assert_ok attribution_names_agent "$tool <x@$domain>"
        assert_ok attribution_names_agent "${tool^^} <x@${domain^^}>"
        assert_fails attribution_names_agent "$tool <x@not$domain>"
        assert_fails attribution_names_agent "$tool <x@mail.$domain>"
        assert_fails attribution_names_agent "$tool <x@$domain.example>"
        assert_fails attribution_names_agent "$tool Monet <x@$domain>"
    done
    for g in "${T_given[@]}"; do
        for v in "${T_vendor[@]}"; do
            [[ "${v%% *}" == "$g" ]] && continue
            # a domain that is another tool's vendor, and not this one's
            if ! _is_in "$g ${v#* }" "${T_vendor[@]}"; then
                assert_fails attribution_names_agent "$g <x@${v#* }>"
            fi
        done
    done
}

#[test]
it_reads_a_name_the_caller_passes_whatever_the_mailbox() {
    local i g
    for i in "${!T_keyed_name[@]}"; do
        # the default lets the row through, and the name is what stops it
        assert_fails attribution_names_agent "${T_keyed_identity[i]}"
        assert_fails attribution_names_agent "${T_keyed_identity[i]}" ''
        assert_ok attribution_names_agent "${T_keyed_identity[i]}" "${T_keyed_name[i]}"
        assert_ok attribution_names_agent "Co-authored-by: ${T_keyed_identity[i]}" "${T_keyed_name[i]}"
        assert_ok attribution_names_agent "--author=\"${T_keyed_identity[i]}\"" "${T_keyed_name[i]}"
    done
    for g in "${T_given[@]}"; do
        assert_fails attribution_names_agent "$g <root@buildhost.local>"
        assert_ok attribution_names_agent "$g <root@buildhost.local>" "$g"
        assert_ok attribution_names_agent "$g <root@buildhost.local>" "${g^^}"
        # a list of names, split at a bar, a comma or a line break
        assert_ok attribution_names_agent "$g <root@buildhost.local>" "Zed|$g"
        assert_ok attribution_names_agent "$g <root@buildhost.local>" "Zed, $g"
        assert_ok attribution_names_agent "$g <root@buildhost.local>" $'Zed\n'"$g"
        # another name names another identity, and an empty entry names nobody
        assert_fails attribution_names_agent "$g <root@buildhost.local>" "Zed"
        assert_fails attribution_names_agent "$g <root@buildhost.local>" "|,"
        # the whole name, not its start, and a group in parentheses is no part of it
        assert_fails attribution_names_agent "$g Monet <root@buildhost.local>" "$g"
        assert_ok attribution_names_agent "$g (she/her) <root@buildhost.local>" "$g"
    done
    # a name the caller passes is any name, and the other nets still hold
    assert_ok attribution_names_agent 'Build Host <root@buildhost.local>' 'Build Host'
    assert_fails attribution_names_agent 'Build Host <root@buildhost.local>' 'Build'
    assert_ok attribution_names_agent 'Copilot <x@example.com>' 'Zed'
    assert_fails attribution_names_agent 'Jane Smith <jane@example.com>' 'Zed'
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
it_keeps_nothing_of_one_identity_for_the_next() {
    # The mailbox is read into globals, and an identity with no mailbox must not
    # be judged on the last one's. These are direct calls, in this shell, so a
    # leak would reach the assertions after them.
    attribution_names_agent 'Claude <claude@localhost>'
    assert_fails attribution_names_agent 'Claude <claude@example.org>'
    attribution_names_agent 'Claude <12345+claude@users.noreply.github.com>'
    assert_fails attribution_names_agent 'Claude <claude@example.org>'
    attribution_names_agent 'Claude <claude@anthropic.com>'
    assert_fails attribution_names_agent 'Claude'
    assert_fails attribution_names_agent 'Claude <c.dupont@example.com>'
    attribution_names_agent 'Claude <root@buildhost.local>' 'Claude'
    assert_fails attribution_names_agent 'Claude <root@buildhost.local>'
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
