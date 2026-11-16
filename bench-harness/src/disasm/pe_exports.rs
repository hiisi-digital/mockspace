//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A named function of a PE image, found through its export table.
//!
//! A linked DLL has no COFF symbol table. `llvm-objdump` labels its functions
//! from the export table; GNU `objdump` does not, and `dumpbin` labels them
//! only from a PDB, which a DLL built elsewhere does not bring. So for those two
//! the function is the stretch of the whole disassembly from its export's
//! address to the next export's. The export table is read by one reader per
//! tool, GNU `objdump -p` and `dumpbin /HEADERS /EXPORTS`, into one [`Exports`];
//! the stretch is cut the same way for both. The last export runs to the end of
//! its section, private functions placed after it included.

/// A PE image's base address and each export's name and RVA.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Exports {
    base:    u64,
    entries: Vec<(String, u64)>,
}

fn hex(s: &str) -> Option<u64> {
    u64::from_str_radix(s.trim(), 16).ok()
}

impl Exports {
    /// From GNU `objdump -p`: `ImageBase`, the export address table by
    /// ordinal index, and the name pointer table naming each index.
    pub(super) fn from_gnu(p: &str) -> Option<Self> {
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
                    if words.next() == Some("RVA")
                        && words.next() == Some("Export")
                        && let Some(rva) = words.next().and_then(hex)
                    {
                        rvas.push((i, rva));
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
        let entries = names
            .into_iter()
            .filter_map(|(i, name)| {
                rvas.iter()
                    .find(|(j, _)| *j == i)
                    .map(|(_, rva)| (name, *rva))
            })
            .collect();
        Some(Self {
            base,
            entries,
        })
    }

    /// From `dumpbin /HEADERS /EXPORTS`: the `image base` line of the optional
    /// header, and each `ordinal hint RVA name` row of the export table.
    /// Written from dumpbin's documented layout; no real dumpbin output has
    /// been read here.
    pub(super) fn from_dumpbin(text: &str) -> Option<Self> {
        let base = text.lines().find_map(|l| {
            let (value, rest) = l.trim().split_once(char::is_whitespace)?;
            rest.trim_start()
                .starts_with("image base")
                .then(|| hex(value))?
        })?;
        let mut entries = Vec::new();
        let mut in_table = false;
        for line in text.lines() {
            let words: Vec<&str> = line.split_whitespace().collect();
            if words == ["ordinal", "hint", "RVA", "name"] {
                in_table = true;
                continue;
            }
            if !in_table {
                continue;
            }
            if words.first() == Some(&"Summary") {
                break;
            }
            if let [ordinal, _hint, rva, name, ..] = words.as_slice()
                && ordinal.parse::<u32>().is_ok()
                && let Some(rva) = hex(rva)
            {
                entries.push((name.to_string(), rva));
            }
        }
        Some(Self {
            base,
            entries,
        })
    }

    /// The lines of `whole`, a disassembly of the image whose lines open with
    /// `<address>:`, from `name`'s export to the next export above it, or
    /// `None` where there is no such export or no line falls in the range.
    pub(super) fn block(&self, whole: &str, name: &str) -> Option<String> {
        let rva = self.entries.iter().find(|(n, _)| n == name)?.1;
        let stop = self
            .entries
            .iter()
            .map(|(_, r)| *r)
            .filter(|r| *r > rva)
            .min();
        let (start, stop) = (self.base + rva, stop.map(|r| self.base + r));
        let lines: Vec<&str> = whole
            .lines()
            .filter(|l| {
                let Some((addr, _)) = l.trim_start().split_once(':') else { return false };
                hex(addr).is_some_and(|a| a >= start && stop.is_none_or(|s| a < s))
            })
            .collect();
        (!lines.is_empty()).then(|| lines.join("\n"))
    }
}
