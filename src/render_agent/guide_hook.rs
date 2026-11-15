//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The `mockspace-guide.sh` hook: at the moment a session reaches for a bench,
//! a sketch or a test, it says once how mockspace does that one, and which
//! skill carries the rest.
//!
//! Never a deny. The rules say the same thing from the start of the session;
//! this is the reminder at the moment of the act, which a rule loaded an hour
//! earlier does not give. Each subject is said once per session per hour.

use super::*;

/// What wakes each subject, and what it says. The hook's own words, in one
/// place, so the test reads them from here rather than from a copy.
pub(crate) const GUIDE_SUBJECTS: &[(&str, &str)] = &[
    (
        "bench",
        "MOCKSPACE BENCHES: no timing loop of your own. 'cargo mock bench add <bench> <arm>', code in <bench>/arms/<arm>/src/lib.rs, 'cargo mock bench run <bench>'; runner logic goes in @MOCK@/benches/src/lib.rs. Skill: benchmarking.",
    ),
    (
        "sketch",
        "MOCKSPACE SKETCHES: probe in @MOCK@/research/sketches/<YYYYMMDDHHMM>_<slug>/, under the repository's toolchain, with FINDINGS.md '**Outcome:**' WORKS / FAILS WITH <error> / INCONCLUSIVE. Skill: sketching.",
    ),
    (
        "test",
        "MOCKSPACE TESTS: 'cargo mock test', not cargo test. One concern per tests/<name>.rs, helpers via 'mod common;', fixtures by literal path. Skill: testing.",
    ),
];

/// Generate the mockspace-guide.sh hook content.
pub(crate) fn builtin_guide(cfg: &Config) -> String {
    let mock_rel = cfg
        .mock_dir
        .strip_prefix(&cfg.repo_root)
        .unwrap_or(&cfg.mock_dir)
        .to_string_lossy()
        .to_string();
    let said = |subject: &str| {
        GUIDE_SUBJECTS
            .iter()
            .find(|(s, _)| *s == subject)
            .map(|(_, m)| m.replace("@MOCK@", &mock_rel))
            .unwrap_or_default()
    };
    r##"#!/usr/bin/env bash
# Built-in mockspace hook: say how mockspace benches, sketches and tests, at the moment one is reached for.
# Non-blocking. Each subject is said once per session, and again after an hour.
set -uo pipefail
__INPUT=$(cat)
{{HOOK_HELPERS}}
FILE_PATH=$(_extract file_path)
COMMAND=$(_extract command)
CONTENT="$(_extract content)$(_extract new_string)"
_scope_or_allow
SUBJECT=""
case "$FILE_PATH" in
    */@MOCK@/benches/*) SUBJECT=bench ;;
    */@MOCK@/research/sketches/*) SUBJECT=sketch ;;
    */tests/*|*_tests.rs) SUBJECT=test ;;
esac
if [[ -z "$SUBJECT" && -n "$FILE_PATH" ]] && [[ "$CONTENT" == *"Instant::now"* ]]; then
    case "$FILE_PATH" in
        */tests/*|*_tests.rs|*/research/*) SUBJECT=bench ;;
    esac
fi
if [[ -z "$SUBJECT" && -n "$COMMAND" ]]; then
    if echo "$COMMAND" | grep -qE '(^|[;&|( ])cargo +(mock +)?bench( |$)'; then
        SUBJECT=bench
    elif echo "$COMMAND" | grep -qE '(^|[;&|( ])rustc( |$)|research/sketches'; then
        SUBJECT=sketch
    elif echo "$COMMAND" | grep -qE '(^|[;&|( ])cargo +(test|nextest)( |$)|(^|[;&|( ])cargo +mock +test( |$)'; then
        SUBJECT=test
    fi
fi
if [[ -z "$SUBJECT" ]]; then
    allow
fi
SESSION_ID=$(echo "$__INPUT" | jq -r '.session_id // ""' 2>/dev/null || echo "")
if [[ "$SESSION_ID" =~ ^[A-Za-z0-9_-]+$ ]]; then
    SAID_DIR="${TMPDIR:-/tmp}/mockspace-guide"
    mkdir -p "$SAID_DIR" 2>/dev/null || true
    find "$SAID_DIR" -type f -mmin +1440 -delete 2>/dev/null || true
    if [[ -n "$(find "$SAID_DIR/$SESSION_ID.$SUBJECT" -mmin -60 2>/dev/null)" ]]; then
        allow
    fi
    : > "$SAID_DIR/$SESSION_ID.$SUBJECT" 2>/dev/null || true
fi
case "$SUBJECT" in
    bench) context "@BENCH@" ;;
    sketch) context "@SKETCH@" ;;
    test) context "@TEST@" ;;
esac
"##
    .replace("@BENCH@", &said("bench"))
    .replace("@SKETCH@", &said("sketch"))
    .replace("@TEST@", &said("test"))
    .replace("@MOCK@", &mock_rel)
}

