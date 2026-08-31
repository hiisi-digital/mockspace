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

use toml log

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
    _mock_resolve || return 1
    printf '%s' "$_MOCK_ROOT"
}

# _mock_launcher
#
# The launcher binary, preferring the direct `mock` over the cargo subcommand
# form so a stale cargo alias cannot shadow it.
_mock_launcher() {
    if command -v mock >/dev/null 2>&1; then
        printf 'mock'
    elif command -v cargo-mock >/dev/null 2>&1; then
        printf 'cargo-mock mock'
    else
        return 1
    fi
}

# _mock_resolve
#
# Ask the launcher where this checkout keeps its mockspace, and cache the
# answer. Sets _MOCK_ROOT, _MOCK_DIR and _MOCK_CONFIG, all absolute, with
# _MOCK_CONFIG empty when the repository has a mock directory and no config.
#
# The search is deliberately NOT reimplemented here. `mock locate` prints what
# `discover::locate` decides, which is the authority: it searches the root and
# then subdirectories hidden-first, handles `mock_dir = "."`, reads the key
# with a parser that stops at the first table so a sectioned `mock_dir` is not
# mistaken for the top-level one, and hard-errors when a repository has more
# than one `mockspace.toml`. The git hooks already carry one shell
# reimplementation of that which has to be kept in step; a second copy here is
# how the three drift apart.
_mock_resolve() {
    [[ -n "$_MOCK_DIR" || -n "$_MOCK_ROOT" ]] && return 0

    local launcher out line
    launcher="$(_mock_launcher)" || {
        log_error "mockspace launcher not found on PATH (install \`mock\`)"
        return 1
    }

    out="$($launcher locate)" || return 1

    while IFS= read -r line; do
        case "$line" in
            root=*)     _MOCK_ROOT="${line#root=}" ;;
            config=*)   _MOCK_CONFIG="${line#config=}" ;;
            mock_dir=*) _MOCK_DIR="${line#mock_dir=}" ;;
        esac
    done <<< "$out"

    [[ -n "$_MOCK_ROOT" ]] || return 1
    return 0
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
    # Empty when the repository has no mock directory at all, which is a repo
    # that does not run mockspace rather than one configured oddly.
    [[ -n "$_MOCK_DIR" ]] || return 1
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
    _mock_resolve || return 1
    [[ -n "$_MOCK_DIR" ]]
}

# mock_get <key> [default]
#
# A value from the governing mockspace.toml. Dotted keys reach into sections.
#
# Usage: name="$(mock_get project_name fallback)"
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
    local suffix="${1:-}" rounds f prior
    rounds="$(mock_rounds_dir)" || return 1
    [[ -d "$rounds" ]] || return 0
    prior=$(shopt -p nullglob)
    shopt -s nullglob
    for f in "${rounds}"/*${suffix}; do
        [[ -f "$f" ]] && printf '%s\n' "$f"
    done
    eval "$prior"
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
