//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The name an arm goes by, read off the file name of its library.
//!
//! A library's prefix and suffix belong to the target it was built for, not to
//! the host reading the path: a manifest written on macOS names `libx.dylib`,
//! a Windows build names `x.dll`, and a Linux host may be handed either. So the
//! naming is taken from the file's own suffix, through the one table below,
//! and never from `std::env::consts::DLL_PREFIX` / `DLL_SUFFIX`, which describe
//! only the host.

/// Every library suffix an arm can carry, with the prefix the targets that use
/// that suffix put in front of a cdylib's name. Windows names a cdylib without
/// a prefix, under both its MSVC and its GNU toolchain. This is a fixed
/// vocabulary of object formats, not content.
const LIBRARY_NAMING: [(&str, &str); 3] = [(".so", "lib"), (".dylib", "lib"), (".dll", "")];

/// The arm name a library path or bare entry carries: the last path component,
/// with a known library suffix and the prefix that suffix's targets add taken
/// off. Either separator ends a component, since a manifest written on Windows
/// may arrive anywhere. An entry with no library suffix is a bare name and is
/// returned whole.
pub fn arm_name(entry: &str) -> &str {
    let file = entry.rsplit(['/', '\\']).next().unwrap_or(entry);
    for (suffix, prefix) in LIBRARY_NAMING {
        if let Some(stem) = file.strip_suffix(suffix) {
            return stem.strip_prefix(prefix).unwrap_or(stem);
        }
    }
    file
}

#[cfg(test)]
mod tests {
    use super::arm_name;

    #[test]
    fn every_target_naming_gives_the_same_arm_on_every_host() {
        for path in [
            "/b/variants/alpha/target/release/libalpha.so",
            "/b/variants/alpha/target/release/libalpha.dylib",
            "C:\\b\\variants\\alpha\\target\\release\\alpha.dll",
            "variants/alpha/target/x86_64-pc-windows-msvc/release/alpha.dll",
            "libalpha.dylib",
            "alpha.dll",
        ] {
            assert_eq!(arm_name(path), "alpha", "{path}");
        }
    }

    #[test]
    fn a_dll_keeps_a_name_that_starts_with_lib() {
        // Windows adds no prefix, so `lib` in a dll's name is the arm's own.
        assert_eq!(arm_name("liblzma.dll"), "liblzma");
        assert_eq!(arm_name("libliblzma.so"), "liblzma");
    }

    #[test]
    fn an_entry_without_a_library_suffix_is_returned_whole() {
        assert_eq!(arm_name("../target/release/multiply_xor"), "multiply_xor");
        assert_eq!(arm_name("plain"), "plain");
        assert_eq!(arm_name("libplain"), "libplain");
        assert_eq!(arm_name("x.rlib"), "x.rlib");
        assert_eq!(arm_name(""), "");
    }

    #[test]
    fn prefix_and_suffix_alone_leave_nothing() {
        assert_eq!(arm_name("lib.so"), "");
        assert_eq!(arm_name(".dll"), "");
    }
}
