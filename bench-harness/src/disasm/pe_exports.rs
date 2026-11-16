//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A named function of a PE image, for GNU `objdump`.
//!
//! A linked DLL has no COFF symbol table. `llvm-objdump` labels its functions
//! from the export table; GNU `objdump` does not, so its `--disassemble=<sym>`
//! finds nothing in a DLL and its whole output carries one `<.text>` label.
//! The export table it prints with `-p` gives each export's RVA, and the
//! function is the stretch of the whole disassembly from its RVA to the next
//! export's. The last export runs to the end of its section, private
//! functions placed after it included.

/// The image base and each export's name and RVA, from GNU `objdump -p`.
pub(super) fn gnu_exports(p: &str) -> Option<(u64, Vec<(String, u64)>)> {
    let hex = |s: &str| u64::from_str_radix(s.trim(), 16).ok();
    let base = p
        .lines()
        .find_map(|l| l.trim().strip_prefix("ImageBase").and_then(hex))?;
    let index = |l: &str| -> Option<usize> {
        let inner = l.trim().strip_prefix('[')?.split_once(']')?.0;
        inner.trim().parse().ok()
    };
    let mut rvas: Vec<(usize, u64)> = Vec::new();
    let mut names: Vec<(usize, String)> = Vec::new();
    let mut section = "";
    for line in p.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Export Address Table --") {
            section = "rva";
            continue;
        }
        if trimmed.starts_with("[Ordinal/Name Pointer] Table") {
            section = "name";
            continue;
        }
        if trimmed.is_empty() {
            section = "";
            continue;
        }
        let Some(i) = index(trimmed) else { continue };
        match section {
            "rva" => {
                let mut words = trimmed.split_whitespace().rev();
                if words.next() == Some("RVA") && words.next() == Some("Export") {
                    if let Some(rva) = words.next().and_then(hex) {
                        rvas.push((i, rva));
                    }
                }
            },
            "name" => {
                if let Some(name) = trimmed.rsplit(']').next() {
                    names.push((i, name.trim().to_string()));
                }
            },
            _ => {},
        }
    }
    let exports = names
        .into_iter()
        .filter_map(|(i, name)| {
            rvas.iter()
                .find(|(j, _)| *j == i)
                .map(|(_, rva)| (name, *rva))
        })
        .collect();
    Some((base, exports))
}

/// The lines of `whole` (GNU `objdump -d` of the image) whose address lies
/// from `name`'s export to the next export above it, or `None` where `p` has
/// no such export or no line falls in the range.
pub(super) fn export_block(whole: &str, p: &str, name: &str) -> Option<String> {
    let (base, exports) = gnu_exports(p)?;
    let rva = exports.iter().find(|(n, _)| n == name)?.1;
    let stop = exports.iter().map(|(_, r)| *r).filter(|r| *r > rva).min();
    let (start, stop) = (base + rva, stop.map(|r| base + r));
    let lines: Vec<&str> = whole
        .lines()
        .filter(|l| {
            let Some((addr, _)) = l.trim_start().split_once(':') else { return false };
            u64::from_str_radix(addr, 16).is_ok_and(|a| a >= start && stop.is_none_or(|s| a < s))
        })
        .collect();
    (!lines.is_empty()).then(|| lines.join("\n"))
}
