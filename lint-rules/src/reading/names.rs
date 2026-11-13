//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The text a path is known by in a key or a trace, one text for one path.
//!
//! A path that is text is its own text. One that is not, a file name with bytes no encoding reads,
//! is a NUL and its bytes in hex, which no path that is text can be, since no path holds a NUL. Two
//! paths that are not text therefore never share a text, as they would if each were made into text
//! by putting a stand-in character for what does not read.

use std::borrow::Cow;
use std::ffi::OsStr;
use std::path::PathBuf;

/// The text `name` is known by.
pub(crate) fn text_of(name: &OsStr) -> Cow<'_, str> {
    match name.to_str() {
        Some(text) => Cow::Borrowed(text),
        None => {
            let mut out = String::from("\0");
            for b in name.as_encoded_bytes() {
                out.push_str(&format!("{b:02x}"));
            }
            Cow::Owned(out)
        },
    }
}

/// The path `text` is known by, the inverse of [`text_of`].
pub(crate) fn path_of(text: &str) -> PathBuf {
    let Some(hex) = text.strip_prefix('\0') else { return PathBuf::from(text) };
    let bytes: Option<Vec<u8>> = (0 .. hex.len() / 2)
        .map(|i| hex.get(2 * i .. 2 * i + 2).and_then(|h| u8::from_str_radix(h, 16).ok()))
        .collect();
    match bytes {
        Some(bytes) if hex.len() % 2 == 0 => from_bytes(bytes),
        _ => PathBuf::from(text),
    }
}

#[cfg(unix)]
fn from_bytes(bytes: Vec<u8>) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;
    PathBuf::from(std::ffi::OsString::from_vec(bytes))
}

#[cfg(not(unix))]
fn from_bytes(bytes: Vec<u8>) -> PathBuf {
    // SAFETY: the bytes are what `as_encoded_bytes` gave on this machine, written by `text_of` into
    // a store this machine keeps, so they are an encoding this platform made.
    PathBuf::from(unsafe { std::ffi::OsString::from_encoded_bytes_unchecked(bytes) })
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
}
