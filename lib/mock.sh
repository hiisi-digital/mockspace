#!/usr/bin/env bash
# =============================================================================
# mockspace/mock.sh - the config, and the paths that fall out of it
# =============================================================================
# Part of mockspace. https://github.com/hiisi-digital/mockspace
#
# Reached from a consumer as:
#
#   [deps.mockspace]                    # in the unit's nut.toml
#   git = "https://github.com/hiisi-digital/mockspace.git"
#   ref = "dev"
#
#   use mockspace::mock                 # in the script
#
# Every hook, skill script and one-off that needs to know where a repository
# keeps its mockspace has been re-deriving it, and getting it wrong: the
# directory is named by `mock_dir` and is not always `mock/`, and the config
# itself sits either at the repo root or inside the mock directory depending on
# whether the repository has been relocated. That is three shapes to get right,
# so it is written once here.
#
# Everything is answered against the repository containing the working
# directory, because that is the repository the caller is working in.
# =============================================================================

nut_once || return 0

use toml git log

# The resolution is stable for the life of a process and involves several
# filesystem probes, so it is done once. A caller that has genuinely changed
# repository mid-run calls mock_reset.
_MOCK_ROOT=""
_MOCK_DIR=""
_MOCK_CONFIG=""

# mock_reset
#
# Forget the resolved paths. Only needed by a process that changes repository
# after having already asked something.
#
# Usage: mock_reset
#[pub]
mock_reset() {
    _MOCK_ROOT=""
    _MOCK_DIR=""
    _MOCK_CONFIG=""
}

# mock_root
#
# The repository root, or failure when the working directory is not in one.
#
# Usage: root="$(mock_root)" || return 1
# Prints: an absolute path
#[pub]
mock_root() {
    [[ -n "$_MOCK_ROOT" ]] && { printf '%s' "$_MOCK_ROOT"; return 0; }
    local root
    root="$(git_root)" || return 1
    [[ -z "$root" ]] && return 1
    _MOCK_ROOT="$root"
    printf '%s' "$_MOCK_ROOT"
}

# _mock_resolve
#
# Find the mock directory and the config that governs it, in the three shapes
# that exist. Sets _MOCK_DIR and _MOCK_CONFIG, both absolute.
#
#   1. Config at the repo root, naming the directory: the relocated shape.
#   2. Config inside the directory it governs: the historical in-place shape,
#      where the containing directory IS the mock directory.
#   3. Config at the root naming nothing: conventional `mock/`.
#   4. A `mock/` directory and no config at all. mockspace's own repository is
#      this shape, and a reader that only wants design_rounds is served by it,
#      so the directory is reported and the config is left empty rather than
#      failing the whole resolution.
_mock_resolve() {
    [[ -n "$_MOCK_DIR" ]] && return 0

    local root at_root named candidate
    root="$(mock_root)" || return 1
    at_root="${root}/mockspace.toml"

    if [[ -f "$at_root" ]]; then
        named="$(toml_get_or "$at_root" "mock_dir" "")"
        if [[ -n "$named" && -d "${root}/${named}" ]]; then
            _MOCK_DIR="${root}/${named}"
            _MOCK_CONFIG="$at_root"
            return 0
        fi
    fi

    # In place: the config sits inside the workspace it governs. Only one level
    # down is searched, because a deeper hit would be some other repository's
    # vendored copy rather than this one's.
    for candidate in "${root}"/*/mockspace.toml; do
        [[ -f "$candidate" ]] || continue
        _MOCK_DIR="${candidate%/mockspace.toml}"
        _MOCK_CONFIG="$candidate"
        return 0
    done

    if [[ -d "${root}/mock" ]]; then
        _MOCK_DIR="${root}/mock"
        [[ -f "$at_root" ]] && _MOCK_CONFIG="$at_root"
        return 0
    fi

    return 1
}

# mock_dir
#
# The mockspace workspace directory, whatever it is named.
#
# Usage: dir="$(mock_dir)" || { log_error "not a mockspace repo"; return 1; }
# Prints: an absolute path
#[pub]
mock_dir() {
    _mock_resolve || return 1
    printf '%s' "$_MOCK_DIR"
}

# mock_config
#
# The mockspace.toml that governs this repository, wherever it sits. Fails when
# there is a mock directory but no config, which is a real shape: mockspace's
# own repository has one.
#
# Usage: cfg="$(mock_config)" || return 1
# Prints: an absolute path
#[pub]
mock_config() {
    _mock_resolve || return 1
    [[ -n "$_MOCK_CONFIG" ]] || return 1
    printf '%s' "$_MOCK_CONFIG"
}

# mock_is_mockspace
#
# Whether the working directory is inside a repository that runs mockspace.
# The question every script that might be run anywhere has to ask first.
#
# Usage: mock_is_mockspace || { log_info "not a mockspace repo"; exit 0; }
#[pub]
mock_is_mockspace() {
    _mock_resolve
}

# mock_get <key> [default]
#
# A value from the governing mockspace.toml. Dotted keys reach into sections.
#
# Usage: name="$(mock_get project_name arvo)"
# Prints: the value, or the default when absent
#[pub]
mock_get() {
    local key="${1:-}" fallback="${2:-}" cfg
    cfg="$(mock_config)" || { printf '%s' "$fallback"; return 1; }
    toml_get_or "$cfg" "$key" "$fallback"
}

# mock_agent_config
#
# The agent config, `<mock_dir>/agent/config.toml`. Present only in a
# repository that has adopted the agent surface, so callers check.
#
# Usage: [[ -f "$(mock_agent_config)" ]] && ...
# Prints: an absolute path, whether or not it exists
#[pub]
mock_agent_config() {
    local dir
    dir="$(mock_dir)" || return 1
    printf '%s/agent/config.toml' "$dir"
}

# mock_agent_get <key> [default]
#
# A value from the agent config. Absent file yields the default rather than a
# failure, because most agent settings are opt-in and absence means "default".
#
# Usage: days="$(mock_agent_get design_talk.stale_topic_days 1)"
# Prints: the value, or the default
#[pub]
mock_agent_get() {
    local key="${1:-}" fallback="${2:-}" cfg
    cfg="$(mock_agent_config)" || { printf '%s' "$fallback"; return 0; }
    [[ -f "$cfg" ]] || { printf '%s' "$fallback"; return 0; }
    toml_get_or "$cfg" "$key" "$fallback"
}

# mock_rounds_dir / mock_crates_dir / mock_agent_dir / mock_docs_dir
#
# The directories a consumer actually reaches for. Each is derived rather than
# assumed, so a repository that renamed its mock directory is served correctly.
#
# Usage: rounds="$(mock_rounds_dir)" || return 1
# Prints: an absolute path
#[pub]
mock_rounds_dir() {
    local dir; dir="$(mock_dir)" || return 1
    printf '%s/design_rounds' "$dir"
}

#[pub]
mock_crates_dir() {
    local dir; dir="$(mock_dir)" || return 1
    printf '%s/crates' "$dir"
}

#[pub]
mock_agent_dir() {
    local dir; dir="$(mock_dir)" || return 1
    printf '%s/agent' "$dir"
}

# Generated documents land at the repo root, not inside the mock directory.
#[pub]
mock_docs_dir() {
    local root; root="$(mock_root)" || return 1
    printf '%s/docs' "$root"
}

# mock_flat_rounds <glob-suffix>
#
# Files matching a suffix directly in design_rounds, which is where an OPEN
# round lives. A timestamped subdirectory is an archive of a closed round and
# is deliberately not matched.
#
# Usage: while read -r f; do ...; done < <(mock_flat_rounds '_topic.*.md')
# Prints: one absolute path per line, nothing when there are none
#[pub]
mock_flat_rounds() {
    local suffix="${1:-}" rounds f
    rounds="$(mock_rounds_dir)" || return 1
    [[ -d "$rounds" ]] || return 0
    shopt -s nullglob
    for f in "${rounds}"/*${suffix}; do
        [[ -f "$f" ]] && printf '%s\n' "$f"
    done
    shopt -u nullglob
}

# mock_phase
#
# The round's phase, derived the way mockspace v1 derives it: from which flat
# files exist and what their suffixes are. There is no stored phase field.
#
#   none    no open round
#   topic   topics only, no changelist
#   doc     an unlocked doc changelist, so doc templates are editable
#   draft   doc changelist locked, no src changelist yet
#   impl    an unlocked src changelist, so source is editable
#   closed  both locked, awaiting close
#
# Usage: [[ "$(mock_phase)" == topic ]] || return 1
# Prints: one of the words above
#[pub]
mock_phase() {
    local doc_open doc_lock src_open src_lock topics
    doc_open="$(mock_flat_rounds '_changelist.doc.md')"
    doc_lock="$(mock_flat_rounds '_changelist.doc.lock.md')"
    src_open="$(mock_flat_rounds '_changelist.src.md')"
    src_lock="$(mock_flat_rounds '_changelist.src.lock.md')"
    topics="$(mock_flat_rounds '_topic.*.md')"

    if   [[ -n "$src_lock" && -n "$doc_lock" ]]; then printf 'closed'
    elif [[ -n "$src_open" ]];                   then printf 'impl'
    elif [[ -n "$doc_lock" ]];                   then printf 'draft'
    elif [[ -n "$doc_open" ]];                   then printf 'doc'
    elif [[ -n "$topics" ]];                     then printf 'topic'
    else                                              printf 'none'
    fi
}
