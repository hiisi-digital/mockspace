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
# ## One library, in parts
#
# The engine is one sourced library to its callers and is written in parts by
# concern, loaded here in the order they depend on each other: `shape.sh` for the
# trailer net, `identity.sh` for what an author, a committer or a co-author
# carries, `adverts.sh` for the suffixes tools append, and `policy.sh` for the
# caller's allow pattern and the scan of a whole message. This file holds the
# self-check, which reaches into all of them.
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

# The parts sit in a directory of the library's own name, found from this file
# and not from the working directory or the entry point.
_ATTRIBUTION_PARTS="$(nut_dir)/attribution"
source "${_ATTRIBUTION_PARTS}/shape.sh"
source "${_ATTRIBUTION_PARTS}/identity.sh"
source "${_ATTRIBUTION_PARTS}/adverts.sh"
source "${_ATTRIBUTION_PARTS}/policy.sh"

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
             ATTRIBUTION_AGENT_TAGS ATTRIBUTION_AGENT_TOOLS ATTRIBUTION_AGENT_GIVEN \
             ATTRIBUTION_AGENT_VENDORS ATTRIBUTION_AGENT_MACHINES ATTRIBUTION_AGENT_HEADS \
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
    attribution_names_agent 'Copilot Chat' || {
        log_error "attribution: the identity net does not recognise a tool followed by a companion"; return 1; }
    attribution_names_agent 'Jane Doe (aider)' || {
        log_error "attribution: the identity net does not recognise a tag"; return 1; }
    attribution_names_agent 'Claude <a@anthropic.com>' || {
        log_error "attribution: the identity net does not recognise a given name at its vendor"; return 1; }
    attribution_names_agent 'Claude <claude@localhost>' || {
        log_error "attribution: the identity net does not recognise a given name by its local part at a machine"; return 1; }
    attribution_names_agent 'Claude <a@b.c>' 'Claude' || {
        log_error "attribution: the identity net does not recognise a name the caller passes"; return 1; }
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
    if attribution_names_agent 'Claude <claude@example.org>'; then
        log_error "attribution: the identity net reads a person called Claude at an ordinary domain as an agent"
        return 1
    fi
    if attribution_names_agent 'Cline Smith'; then
        log_error "attribution: the identity net reads a tool followed by a name as an agent"
        return 1
    fi
    if attribution_names_agent 'Jane Doe (OpenAI)'; then
        log_error "attribution: the identity net reads a group that is no tag as an agent"
        return 1
    fi
    if attribution_names_agent 'Claude Max <a@b.c>' 'Devin'; then
        log_error "attribution: the identity net reads a name the caller did not pass as an agent"
        return 1
    fi
    return 0
}
