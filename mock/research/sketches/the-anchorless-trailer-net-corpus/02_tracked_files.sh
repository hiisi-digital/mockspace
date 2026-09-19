#!/bin/sh
# P2. The same question over a different corpus: tracked file content.
#
# `attribution_is_attribution_trailer` is called on markdown as well as on
# commit messages, which is the reason `>`, `-`, `|`, `*` and a leading dot stay
# in rather than coming off with the whitespace and the hashes. So the corpus
# that decides what the widening costs is not only what people wrote in commit
# messages, and `01_commit_messages.sh` on its own answers half the question.
#
# CORPUS. The tracked content of `$2`, default `HEAD`, in `$1`, default the
# repository this script sits in. One ref rather than every ref: file content is
# a tree rather than a history, and a claim about what the net does today is a
# claim about today's tree.
#
# NEGATIVE CONTROL, stated before the run. The same two planted lines as P1,
# written into a file inside a scratch repository and committed there: the
# indented byline must be newly matched and the ordinary subject must not.
# Run against the scratch repository rather than against the real one, so the
# control cannot pass on something the corpus happened to carry.
#
# Usage: ./02_tracked_files.sh [<repo> [<ref>]]
#
# Copyright (c) 2026 orgrinrt <ort@hiisi.digital>
# SPDX-License-Identifier: MPL-2.0

set -eu

repo="${1:-$(cd "$(dirname "$0")" && git rev-parse --show-toplevel)}"
ref="${2:-HEAD}"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

KEY='([A-Za-z]+-)*[A-Za-z]+-(by|session|agent|model|tool)|author|committer|generated-with'
WIDE="^[[:space:]#]*(${KEY}):[[:space:]]*[^[:space:]]"
ANCH="^(${KEY}):[[:space:]]*[^[:space:]]"

# `ref:path:line:content` from git grep, reduced to the address, so the two
# sets can be differenced without the content getting in the way.
hits() {
    git -C "$1" grep -I -n -iE "$3" "$2" -- . 2>/dev/null | cut -d: -f1-3 | sort -u
}

scratch="$tmp/control"
git init -q "$scratch"
git -C "$scratch" config user.name t
git -C "$scratch" config user.email t@t
git -C "$scratch" config core.hooksPath /dev/null
{ printf '    # Co-Authored-By: Jane Roe <jane@example.com>\n'
  printf 'fix: a thing\n'
} > "$scratch/planted.md"
git -C "$scratch" add planted.md
git -C "$scratch" commit -qm "test: the planted corpus"

hits "$scratch" HEAD "$WIDE" > "$tmp/c.wide"
hits "$scratch" HEAD "$ANCH" > "$tmp/c.anch"
comm -23 "$tmp/c.wide" "$tmp/c.anch" > "$tmp/c.newly"
if [ "$(wc -l < "$tmp/c.newly" | tr -d ' ')" != "1" ]; then
    printf 'control failed: the planted marked byline was not newly matched\n'
    cat "$tmp/c.wide" "$tmp/c.anch"
    exit 1
fi
printf 'control held: the marked byline is newly matched, the subject is not\n\n'

hits "$repo" "$ref" "$WIDE" > "$tmp/wide"
hits "$repo" "$ref" "$ANCH" > "$tmp/anch"
comm -23 "$tmp/wide" "$tmp/anch" > "$tmp/newly"

printf 'repo:     %s\n' "$repo"
printf 'ref:      %s (%s)\n' "$ref" "$(git -C "$repo" rev-parse --short "$ref")"
printf 'anchored: %s\n' "$(wc -l < "$tmp/anch" | tr -d ' ')"
printf 'widened:  %s\n' "$(wc -l < "$tmp/wide" | tr -d ' ')"
printf 'newly:    %s\n\n' "$(wc -l < "$tmp/newly" | tr -d ' ')"

if [ -s "$tmp/newly" ]; then
    printf 'what the widening newly matches, with the line it matched:\n'
    while IFS= read -r addr; do
        path="${addr#*:}"; path="${path%:*}"
        line="${addr##*:}"
        printf '  %s:%s\n    %s\n' "$path" "$line" \
            "$(git -C "$repo" show "$ref:$path" | sed -n "${line}p")"
    done < "$tmp/newly"
else
    printf 'the widening newly matches nothing in this tree\n'
fi
