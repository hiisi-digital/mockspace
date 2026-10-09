#!/usr/bin/env nutshell
# shellcheck shell=bash
# =============================================================================
# attribution_identity_mailbox_test - what the mailbox and the caller add to a name
# =============================================================================
# Run: ./test tests/attribution_identity_mailbox_test.sh
#
# A given-name tool behind the mailbox that is its own is an agent and a person
# called the same at an ordinary domain is not; a name the caller passes is an
# agent whatever the mailbox; and nothing of one identity is kept for the next.
# =============================================================================

use test

# The library, the table and the helpers every identity suite shares.
source "$(dirname "${BASH_SOURCE[0]}")/identity_table.sh"

#[test]
it_reads_a_given_name_behind_a_mailbox_that_is_the_tools() {
    local g v tool domain d
    for g in "${T_given[@]}"; do
        # the local part is the tool's word, whole, at a mailbox that is a
        # machine's: localhost, a domain of one label, or a domain ending in one
        # of the machine endings in the table
        assert_ok attribution_names_agent "$g <$g@localhost>"
        assert_ok attribution_names_agent "$g <$g@buildbox>"
        assert_ok attribution_names_agent "${g^^} <${g^^}@BUILDBOX>"
        for d in "${T_machine[@]}"; do
            assert_ok attribution_names_agent "$g <$g@ci.$d>"
            assert_ok attribution_names_agent "${g^^} <${g^^}@CI.${d^^}>"
            # the ending has to end the domain, and a dot has to come before it
            assert_fails attribution_names_agent "$g <$g@ci.$d.example.com>"
            assert_fails attribution_names_agent "$g <$g@$d.example.com>"
            assert_fails attribution_names_agent "$g <$g@ci$d.example.com>"
        done
        # the same local part at an ordinary domain is a person called that
        assert_fails attribution_names_agent "$g <$g@example.com>"
        assert_fails attribution_names_agent "$g <$g@acme.com>"
        assert_fails attribution_names_agent "$g <$g@gmail.com>"
        assert_fails attribution_names_agent "${g^^} <${g^^}@EXAMPLE.COM>"
        # and so is the private address GitHub gives every account, with the
        # number or without it, the number being no part of the tool's word
        assert_fails attribution_names_agent "$g <12345+$g@users.noreply.github.com>"
        assert_fails attribution_names_agent "${g^^} <12345+${g^^}@USERS.NOREPLY.GITHUB.COM>"
        assert_fails attribution_names_agent "$g <$g@users.noreply.github.com>"
        assert_fails attribution_names_agent "$g <12345+$g@example.com>"
        assert_fails attribution_names_agent "$g <12345+$g@ci.local>"
        # the local part is the tool's word whole
        assert_fails attribution_names_agent "$g <x$g@localhost>"
        assert_fails attribution_names_agent "$g <$g.x@localhost>"
        assert_fails attribution_names_agent "$g <x$g@ci.local>"
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
it_keeps_nothing_of_one_identity_for_the_next() {
    # The mailbox is read into globals, and an identity with no mailbox must not
    # be judged on the last one's. These are direct calls, in this shell, so a
    # leak would reach the assertions after them.
    attribution_names_agent 'Claude <claude@localhost>'
    assert_fails attribution_names_agent 'Claude <claude@example.org>'
    assert_fails attribution_names_agent 'Claude'
    attribution_names_agent 'Claude <claude@buildbox>'
    assert_fails attribution_names_agent 'Claude <claude@example.org>'
    assert_fails attribution_names_agent 'Claude <c.dupont@example.com>'
    attribution_names_agent 'Claude <claude@anthropic.com>'
    assert_fails attribution_names_agent 'Claude'
    assert_fails attribution_names_agent 'Claude <c.dupont@example.com>'
    attribution_names_agent 'Claude <root@buildhost.local>' 'Claude'
    assert_fails attribution_names_agent 'Claude <root@buildhost.local>'
}

