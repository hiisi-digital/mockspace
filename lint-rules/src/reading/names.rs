//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The text a path is known by in a key or a trace, one text for one path.
//!
//! A path that is text is its own text. One that is not, a file name with bytes no encoding reads,
//! is a NUL and its bytes in hex, or on windows its sixteen-bit units in hex, which no path that is
//! text can be, since no path holds a NUL. Two paths that are not text therefore never share a
//! text, as they would if each were made into text by putting a stand-in character for what does
//! not read.

use std::borrow::Cow;
use std::ffi::OsStr;
use std::path::PathBuf;

/// The text `name` is known by.
pub(crate) fn text_of(name: &OsStr) -> Cow<'_, str> {
    match name.to_str() {
        Some(text) => Cow::Borrowed(text),
        None => Cow::Owned(escaped(name)),
    }
}

/// The path `text` is known by, the inverse of [`text_of`]. A text that opens with a NUL and does
/// not decode, a store written elsewhere or damaged, is taken as it reads, a path no file has.
pub(crate) fn path_of(text: &str) -> PathBuf {
    text.strip_prefix('\0').and_then(unescaped).unwrap_or_else(|| PathBuf::from(text))
}

/// The hex digits of `hex` in fours or twos as `width` says, or nothing where any does not decode.
fn units(hex: &str, width: usize) -> Option<Vec<u32>> {
    if hex.is_empty() || hex.len() % width != 0 || !hex.bytes().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    (0 .. hex.len() / width)
        .map(|i| hex.get(width * i .. width * (i + 1)).and_then(|h| u32::from_str_radix(h, 16).ok()))
        .collect()
}

/// A name on unix is bytes, each two hex digits.
#[cfg(unix)]
fn escaped(name: &OsStr) -> String {
    use std::os::unix::ffi::OsStrExt;
    std::iter::once("\0".to_string()).chain(name.as_bytes().iter().map(|b| format!("{b:02x}"))).collect()
}

#[cfg(unix)]
fn unescaped(hex: &str) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    let bytes = units(hex, 2)?.into_iter().map(|u| u as u8).collect();
    Some(PathBuf::from(std::ffi::OsString::from_vec(bytes)))
}

/// A name on windows is sixteen-bit units, each four hex digits, and any sequence of them is a
/// name.
#[cfg(windows)]
fn escaped(name: &OsStr) -> String {
    use std::os::windows::ffi::OsStrExt;
    std::iter::once("\0".to_string()).chain(name.encode_wide().map(|u| format!("{u:04x}"))).collect()
}

#[cfg(windows)]
fn unescaped(hex: &str) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    let wide: Vec<u16> = units(hex, 4)?.into_iter().map(|u| u as u16).collect();
    Some(PathBuf::from(std::ffi::OsString::from_wide(&wide)))
}

/// Elsewhere a name that is not text is known as it reads, and two such names can share a text.
#[cfg(not(any(unix, windows)))]
fn escaped(name: &OsStr) -> String {
    name.to_string_lossy().into_owned()
}

#[cfg(not(any(unix, windows)))]
fn unescaped(_: &str) -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_is_its_own_text_and_comes_back_from_it() {
        assert_eq!(text_of(OsStr::new("src/lib.rs")), "src/lib.rs");
        assert_eq!(path_of("src/lib.rs"), PathBuf::from("src/lib.rs"));
    }

    #[cfg(unix)]
    #[test]
    fn two_paths_that_are_not_text_have_two_texts_and_come_back_from_them() {
        use std::os::unix::ffi::OsStrExt;
        let a = OsStr::from_bytes(b"dir/n\xfe");
        let b = OsStr::from_bytes(b"dir/n\xff");
        assert_eq!(a.to_string_lossy(), b.to_string_lossy(), "the control: made into text, the two read alike");
        assert_ne!(text_of(a), text_of(b));
        assert_eq!(path_of(&text_of(a)), PathBuf::from(a));
        assert_eq!(path_of(&text_of(b)), PathBuf::from(b));
    }

    #[test]
    fn a_text_that_does_not_decode_is_taken_as_it_reads() {
        for damaged in ["\0", "\0zz", "\0abc", "\0+1"] {
            assert_eq!(path_of(damaged), PathBuf::from(damaged), "{damaged:?}");
        }
    }
}
