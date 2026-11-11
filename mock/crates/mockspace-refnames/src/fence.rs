//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The code fences of a Markdown text, inside which nothing is a reference.

/// Where the lines of a Markdown text stand against its code fences, read one line after another.
///
/// A fence opens on a line of three or more backticks, or three or more tildes, after any
/// indentation, so a fence inside a list item counts. It closes on a line of the same character, at
/// least as long as the one that opened it, with nothing after it but whitespace; any other line
/// inside it, a shorter fence or one of the other character among them, is part of it. A backtick
/// fence whose info string holds a backtick opens nothing, as CommonMark reads it. A fence that is
/// never closed runs to the end of the text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Fences {
    open: Option<Fence>,
}

/// The fence a text is inside: its character and how many of it opened it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Fence {
    mark: u8,
    len:  usize,
}

impl Fences {
    /// The state before the first line of a text: outside every fence.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            open: None,
        }
    }

    /// Reads the next line of the text and says whether it is prose: neither a line that opens or
    /// closes a fence, nor a line inside one. The line may carry its line break.
    pub const fn prose(&mut self, line: &str) -> bool {
        let b = line.as_bytes();
        let start = skip_blank(b, 0);
        let (mark, len) = run(b, start);
        match self.open {
            Some(open) => {
                if mark == open.mark && len >= open.len && skip_blank(b, start + len) == b.len() {
                    self.open = None;
                }
                false
            },
            None => {
                let opens = len >= 3 && (mark == b'~' || !holds(b, start + len, b'`'));
                if opens {
                    self.open = Some(Fence {
                        mark,
                        len,
                    });
                }
                !opens
            },
        }
    }
}

/// The index of the first byte at or after `i` that is not a space, a tab or a line break.
const fn skip_blank(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\r' | b'\n') {
        i += 1;
    }
    i
}

/// The fence character at `i` and how many of it follow in a row, or `(0, 0)` where `i` holds
/// neither a backtick nor a tilde.
const fn run(b: &[u8], i: usize) -> (u8, usize) {
    if i >= b.len() || !matches!(b[i], b'`' | b'~') {
        return (0, 0);
    }
    let mark = b[i];
    let mut n = 0;
    while i + n < b.len() && b[i + n] == mark {
        n += 1;
    }
    (mark, n)
}

/// Whether `c` stands anywhere at or after `i`.
const fn holds(b: &[u8], mut i: usize, c: u8) -> bool {
    while i < b.len() {
        if b[i] == c {
            return true;
        }
        i += 1;
    }
    false
}

#[cfg(test)]
#[path = "fence_tests.rs"]
mod tests;
