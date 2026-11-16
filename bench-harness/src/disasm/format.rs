//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The object format of a file on disk, read from its own leading bytes, and
//! what that format means for the spelling of a C symbol.
//!
//! The format is the file's, never the host's: an arm built for another
//! target and handed to this host is read as what it is.

use std::io::Read;
use std::path::Path;

/// The object formats a cdylib or an object file of a supported target comes
/// in. A fixed vocabulary of formats, not content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ObjectFormat {
    /// ELF, Linux and the BSDs. Symbols carry no prefix.
    Elf,
    /// Mach-O, macOS and iOS. Every C symbol carries a leading underscore.
    MachO,
    /// PE/COFF, Windows.
    Coff {
        /// Whether the machine is 32-bit x86, whose COFF symbol table
        /// prefixes a `cdecl` symbol with an underscore.
        i386:  bool,
        /// Whether this is a linked image (a DLL) rather than an object. An
        /// image carries no COFF symbol table; the names a disassembler shows
        /// for it come from the export table, which spells a `cdecl` export
        /// without the underscore.
        image: bool,
    },
}

const COFF_I386: u16 = 0x014C;
const COFF_AMD64: u16 = 0x8664;
const COFF_ARM64: u16 = 0xAA64;

impl ObjectFormat {
    /// The format `bytes` (the start of a file, 64 bytes or more where the
    /// file has them) is in, or `None` where it is none of the three.
    pub(super) fn of_bytes(bytes: &[u8]) -> Option<Self> {
        match bytes {
            [0x7F, b'E', b'L', b'F', ..] => Some(Self::Elf),
            // 32- and 64-bit Mach-O in both byte orders, and a universal file.
            [0xFE, 0xED, 0xFA, 0xCE | 0xCF, ..]
            | [0xCE | 0xCF, 0xFA, 0xED, 0xFE, ..]
            | [0xCA, 0xFE, 0xBA, 0xBE, ..] => Some(Self::MachO),
            [b'M', b'Z', ..] => {
                // A PE image: the COFF header follows `PE\0\0` at `e_lfanew`.
                let at = u32::from_le_bytes(bytes.get(0x3C .. 0x40)?.try_into().ok()?) as usize;
                if bytes.get(at .. at + 4)? != b"PE\0\0" {
                    return None;
                }
                let machine = u16::from_le_bytes(bytes.get(at + 4 .. at + 6)?.try_into().ok()?);
                Some(Self::Coff {
                    i386:  machine == COFF_I386,
                    image: true,
                })
            },
            [lo, hi, ..] => {
                // A bare COFF object starts with its machine.
                let i386 = match u16::from_le_bytes([*lo, *hi]) {
                    COFF_I386 => true,
                    COFF_AMD64 | COFF_ARM64 => false,
                    _ => return None,
                };
                Some(Self::Coff {
                    i386,
                    image: false,
                })
            },
            _ => None,
        }
    }

    /// The format of the file at `path`, or `None` where it cannot be read or
    /// is none of the three. A PE header can sit past the first kilobyte only
    /// in a file no linker of a supported target writes.
    pub(super) fn of_file(path: &Path) -> Option<Self> {
        let mut head = Vec::with_capacity(1024);
        std::fs::File::open(path)
            .ok()?
            .take(1024)
            .read_to_end(&mut head)
            .ok()?;
        Self::of_bytes(&head)
    }

    /// The spellings `symbol` (its name in C) may have in this format, the
    /// likeliest first.
    ///
    /// A 32-bit PE image tries the bare export name before the underscored
    /// one: no i686 DLL has been read here, only synthetic header bytes, so
    /// which of the two a disassembler shows for it is not settled.
    pub(super) fn spellings(self, symbol: &str) -> Vec<String> {
        let bare = symbol.to_string();
        let underscored = format!("_{symbol}");
        match self {
            Self::MachO
            | Self::Coff {
                i386: true,
                image: false,
            } => vec![underscored],
            Self::Coff {
                i386: true,
                image: true,
            } => vec![bare, underscored],
            Self::Elf
            | Self::Coff {
                i386: false,
                ..
            } => vec![bare],
        }
    }
}

/// The spellings to look `symbol` up under in the file at `path`: those its
/// format uses, or, where the format cannot be told, the bare spelling and
/// then the underscore-prefixed one.
pub(super) fn symbol_spellings(path: &Path, symbol: &str) -> Vec<String> {
    match ObjectFormat::of_file(path) {
        Some(format) => format.spellings(symbol),
        None => vec![symbol.to_string(), format!("_{symbol}")],
    }
}
