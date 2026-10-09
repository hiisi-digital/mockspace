#!/usr/bin/env bash
# =============================================================================
# mockspace/attribution/shape.sh - the shape net: which lines attribute work to somebody
# =============================================================================
# Part of the attribution library, sourced by `lib/attribution.sh` and reached
# through `use mockspace::attribution` like the rest of it, not on its own.
# =============================================================================

# -----------------------------------------------------------------------------
# The shape net: which `Key: value` lines attribute work to somebody.
#
# `*-by` covers `Co-authored-by`, `Signed-off-by`, `Assisted-by`, `Reviewed-by`
# and whatever the next one is called. The rest are keys tools have actually
# shipped.
#
# A conventional-commit subject is `Key: value` shaped too, and matching one
# would condemn a repository for a commit about a feature: `docs: describe how
# the copilot integration surface is configured` is not a trailer. So the key
# must be an attribution key rather than merely a word, and the caller should
# prefer a real trailer parser where it has one.
# -----------------------------------------------------------------------------
ATTRIBUTION_TRAILER_KEY_RE='([A-Za-z]+-)*[A-Za-z]+-(by|session|agent|model|tool)|author|committer|generated-with'

# attribution_is_attribution_trailer <line>
#
# Whether a line is structurally an attribution: an attribution key, a colon,
# and a value. Says nothing about who is named or whether it is permitted.
#
# This is the net that keeps working when a new tool ships, so a caller wanting
# one check should want this one.
#
# Whitespace and hashes come off the front first, in any order and any number,
# because the anchor was the hole. A byline behind one leading space failed the
# match and the scan reported nothing, and so did every spelling with a `#` in
# front of it. Measured on git 2.55.0: `git commit -F` stores
# `# # Co-Authored-By: ...` in the body verbatim while git's own trailer parser
# returns nothing for it, so a byline spelled that way passes the parser the
# callers use for commit trailers and passed this net as well. What comes off is
# every arrangement of those two rather than the spellings somebody thought of,
# since the two previous repairs of this shape in the commit-msg hook each named
# a spelling and the next spelling got through.
#
# Whitespace and `#` is the whole of the set here, and it is not the whole of
# what defeats an anchor: `>`, `//`, `-`, `|`, `*` and a leading dot each hide a
# byline from this net as well, and git stores all of them verbatim. Those stay
# in, because this net reads markdown as well as commit messages, where `>` opens
# a quotation and `-` opens a list, and a false refusal here costs somebody a
# rehoused repository rather than one reword. A caller whose surface is only a
# commit message can take them off before it calls, and a caller that wants that
# has to do it: nothing in here does it for one, and a caller that passes a line
# through untouched gets the narrow set above and no more.
#
# What the strip costs, and it is not nothing. `author` and `committer` are in
# the key pattern as bare words, since a git author field has no `-by` shape, so
# with the anchor gone an indented `author: Jane` reads as a trailer. That is a
# markdown code block indented by four spaces, or a YAML document, or a struct
# literal in a diff. `attribution_strip_quoted` knows a fence and an inline
# backtick and does not know an indented block, and the trailer path never calls
# it.
#
# What that costs is measured rather than argued, by the two scripts under
# `mock/research/sketches/the-anchorless-trailer-net-corpus/`, and the answer
# depends on which corpus is asked. Over commit messages, every commit reachable
# from every ref in this repository and in one private workspace repository,
# the widening newly matches nothing. Over tracked file content it newly matches
# three lines here, two of them a struct literal in
# `mock/crates/mockspace-core/src/io/ref_write.rs` whose fields are named
# `author` and `committer`. So the shape is real and somebody can go and read
# it, and nothing refuses over it today because the trailer path is called on
# commit messages and on markdown rather than on rust. Run the scripts rather
# than trusting the counts in this paragraph, which were true when they were
# taken. `it_reads_an_indented_author_line_as_a_trailer` is where the shape is
# written down, and it is a pin on today's answer rather than an endorsement.
#
# A quotation stays safe, because the convention here is to backtick the
# forbidden string and a leading backtick is not stripped. A heading spelling
# out a live key with a real value and no backticks does read as a trailer, and
# that is the trade: the cheaper repair is to backtick it.
#
# Usage: attribution_is_attribution_trailer "$line" && ...
#[pub]
attribution_is_attribution_trailer() {
    local line="${1:-}"
    line="${line#"${line%%[![:space:]#]*}"}"
    printf '%s' "$line" | grep -qiE "^(${ATTRIBUTION_TRAILER_KEY_RE}):[[:space:]]*[^[:space:]]"
}
