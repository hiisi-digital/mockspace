#!/bin/sh
# P1. Over a repository's commit messages, what does taking the anchor off
# `attribution_is_attribution_trailer` newly match?
#
# The widening strips `[[:space:]#]*` off the front of a line before testing it
# against the trailer pattern, so it catches `# Co-Authored-By: ...`, which git
# stores verbatim and which the anchored form missed. The cost is that the key
# pattern carries `author` and `committer` as bare words, so an indented
# `author: Jane` now reads as a trailer where it did not before: a markdown code
# block indented by four spaces, a YAML document, a struct literal in a diff.
#
# This probe answers what that costs on a real corpus rather than in principle.
#
# CORPUS. `$1`, default the repository this script sits in, over `git log --all`,
# which is every commit reachable from every ref in that clone. A shallow clone
# answers about its graft depth and says so: the script refuses one, because
# the claim this exists to support is about whole histories and a shallow clone
# returns a small number that looks like a measurement.
#
# NEGATIVE CONTROL, stated before the run. Two lines are planted into a copy of
# the stream: `  # Co-Authored-By: Jane Roe <jane@example.com>`, which the
# widening must newly match, and `fix: a thing`, which neither form may match.
# If the planted byline does not come back the instrument reports zero for every
# corpus and the run below means nothing, so the script stops there.
#
# Usage: ./01_commit_messages.sh [<repo>]
#
# Copyright (c) 2026 orgrinrt <ort@hiisi.digital>
# SPDX-License-Identifier: MPL-2.0

set -eu

repo="${1:-$(cd "$(dirname "$0")" && git rev-parse --show-toplevel)}"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

KEY='([A-Za-z]+-)*[A-Za-z]+-(by|session|agent|model|tool)|author|committer|generated-with'
PRE='^[0-9a-f]{40} '
WIDE="${PRE}[[:space:]#]*(${KEY}):[[:space:]]*[^[:space:]]"
ANCH="${PRE}(${KEY}):[[:space:]]*[^[:space:]]"

if [ "$(git -C "$repo" rev-parse --is-shallow-repository)" = "true" ]; then
    printf 'refusing: %s is a shallow clone, so its history is not its history\n' "$repo"
    printf 'run `git -C %s fetch --unshallow` first\n' "$repo"
    exit 2
fi

# Every message line, prefixed with the sha it came from, so the pattern can
# still anchor: the prefix is a fixed forty hex digits and a space.
soh="$(printf '\001')"
git -C "$repo" log --all --format="%x01%H%n%B" \
    | awk -v M="$soh" '
        index($0, M) == 1 { sha = substr($0, 2); next }
        { print sha " " $0 }
      ' > "$tmp/lines"

control="0000000000000000000000000000000000000000"
{ printf '%s   # Co-Authored-By: Jane Roe <jane@example.com>\n' "$control"
  printf '%s fix: a thing\n' "$control"
} > "$tmp/control"

grep -iE "$WIDE" "$tmp/control" > "$tmp/control.wide" || true
grep -ivE "$ANCH" "$tmp/control.wide" > "$tmp/control.newly" || true
if [ "$(wc -l < "$tmp/control.newly" | tr -d ' ')" != "1" ]; then
    printf 'control failed: the planted marked byline was not newly matched\n'
    cat "$tmp/control.newly"
    exit 1
fi
if grep -qE "$WIDE" "$tmp/control" && grep -q 'fix: a thing' "$tmp/control.wide"; then
    printf 'control failed: an ordinary subject line was matched\n'
    exit 1
fi
printf 'control held: the marked byline is newly matched, the subject is not\n\n'

grep -iE "$WIDE" "$tmp/lines" > "$tmp/wide" || true
grep -ivE "$ANCH" "$tmp/wide" > "$tmp/newly" || true

printf 'repo:     %s\n' "$repo"
printf 'commits:  %s\n' "$(git -C "$repo" rev-list --all --count)"
printf 'lines:    %s\n' "$(wc -l < "$tmp/lines" | tr -d ' ')"
printf 'anchored: %s\n' "$(grep -icE "$ANCH" "$tmp/lines" || true)"
printf 'widened:  %s\n' "$(wc -l < "$tmp/wide" | tr -d ' ')"
printf 'newly:    %s\n\n' "$(wc -l < "$tmp/newly" | tr -d ' ')"

if [ -s "$tmp/newly" ]; then
    printf 'what the widening newly matches:\n'
    cut -c1-12,41- "$tmp/newly"
else
    printf 'the widening newly matches nothing in this corpus\n'
fi
