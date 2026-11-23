//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;

/// The lines of `text` that are prose, by their number from 1.
fn prose_lines(text: &str) -> Vec<usize> {
    let mut fences = Fences::new();
    text.split_inclusive('\n')
        .enumerate()
        .filter_map(|(i, line)| fences.prose(line).then_some(i + 1))
        .collect()
}

/// One row for each way a line opens, closes or stays inside a fence, and each way it does not.
const TABLE: &[(&str, &str, &[usize])] = &[
    ("no fence", "a\nb\n", &[1, 2]),
    ("backticks", "a\n```\nb\n```\nc\n", &[1, 5]),
    ("tildes", "a\n~~~\nb\n~~~\nc\n", &[1, 5]),
    ("an info string", "```rust\nb\n```\nc\n", &[4]),
    ("an indented fence", "- x\n  ```\n  b\n  ```\nc\n", &[1, 5]),
    ("two backticks open nothing", "``\nb\n", &[1, 2]),
    (
        "a shorter fence inside a longer one",
        "````\n```\nb\n````\nc\n",
        &[5],
    ),
    ("a longer closing fence", "```\nb\n`````\nc\n", &[4]),
    (
        "backticks inside a tilde fence",
        "~~~\n```\nb\n~~~\nc\n",
        &[5],
    ),
    (
        "tildes inside a backtick fence",
        "```\n~~~\nb\n```\nc\n",
        &[5],
    ),
    ("text after a closing fence", "```\n``` x\nb\n```\nc\n", &[
        5,
    ]),
    ("blank after a closing fence", "```\nb\n```  \r\nc\n", &[4]),
    ("a backtick in a backtick info string", "``` a`b\nc\n", &[
        1, 2,
    ]),
    (
        "a backtick in a tilde info string",
        "~~~ a`b\nc\n~~~\nd\n",
        &[4],
    ),
    ("a fence never closed", "a\n```\nb\nc", &[1]),
    ("no line break at the end", "```\nb\n```", &[]),
];

// Mutation: toggling on any line opening with three backticks, as every copy before this did, gets
// the tilde rows, the nested rows, the trailing text row and both info string rows wrong; dropping
// the length check closes a four backtick fence on three; reading a tilde fence's info string for a
// backtick leaves it unopened.
#[test]
fn each_line_is_prose_exactly_where_no_fence_holds_it() {
    for (what, text, want) in TABLE {
        assert_eq!(prose_lines(text), *want, "{what}: {text:?}");
    }
}

// The state is the text's alone: a fresh reader starts outside every fence whatever another was in.
#[test]
fn a_new_reader_starts_outside_every_fence() {
    let mut open = Fences::new();
    assert!(!open.prose("```\n"));
    assert_eq!(Fences::new(), Fences::default());
    assert!(Fences::new().prose("a\n"));
}
