//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Automatic disassembly diffing for variant libraries, and the disassembly
//! of one named function as an artifact.
//!
//! Before timing, compare each variant library's compiled machine code. If
//! two variants produce identical code, [`check_duplicates`] warns: they will
//! benchmark identically, wasting time.
//!
//! The disassembler is found once per process ([`Disassembler::detected`]):
//! `llvm-objdump` first, then the toolchain's `llvm-tools` one, then
//! `objdump` read as LLVM's or GNU's by its own `--version`, then `otool` on
//! macOS and `dumpbin` on Windows. A symbol is spelled as the library's own
//! object format spells it (a leading underscore on Mach-O and 32-bit COFF,
//! none on ELF and 64-bit COFF), and a variant's name comes off its file's
//! own suffix ([`crate::library::arm_name`]), so an arm built for another
//! target reads correctly on any host.
//!
//! The comparison has two layers. `bench_entry` (the FFI dispatcher
//! `#[bench_variant]` generates) is cheap to disassemble in isolation and a
//! *different* `bench_entry` proves the variants differ, since the entry is
//! part of the code. It does not prove the converse: the user's per-size
//! function is called through `bench_entry` rather than always inlined into
//! it, so two variants computing entirely different work can still export a
//! byte-identical dispatcher shell. The authoritative check disassembles
//! every executable section (`otool`: `__TEXT,__text`), which sees the user's
//! function wherever the optimizer put it. One tool reads every variant of a
//! run, so the two are never compared across different byte ranges.
//!
//! When extraction itself fails, silence would look identical to a clean
//! pass: an empty duplicate list either way. [`check_duplicates`] reports that
//! case explicitly (see [`CheckReport`]) rather than letting a variant it
//! could not read vanish into "no duplicates found".

use std::path::Path;

mod artifact;
mod format;
mod normalize;
mod tool;

pub use artifact::{ArtifactPlace, write_symbol_artifact};
use normalize::normalize_disasm;
pub use tool::{Disassembler, Flavour};

/// The normalised disassembly of `bench_entry` in `library`, or `None` where
/// it could not be read or came out empty.
fn extract_bench_entry(tool: &Disassembler, library: &str) -> Option<String> {
    let raw = tool.symbol(Path::new(library), "bench_entry").ok()?;
    non_empty(normalize_disasm(&raw, tool.flavour))
}

/// The normalised disassembly of every executable section of `library`, or
/// `None` where it could not be read or came out empty. An empty result is
/// never evidence: two empties would otherwise compare equal and report a
/// duplicate neither side was seen to be.
fn extract_text_section(tool: &Disassembler, library: &str) -> Option<String> {
    let raw = tool.whole(Path::new(library)).ok()?;
    non_empty(normalize_disasm(&raw, tool.flavour))
}

fn non_empty(text: String) -> Option<String> {
    (!text.trim().is_empty()).then_some(text)
}

/// The disassembly of one function of a library, as the tool printed it, and
/// the tool that printed it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SymbolDisassembly {
    /// The disassembler that read the library.
    pub tool:       Disassembler,
    /// From the function's label to the end of its body, addresses kept and
    /// raw instruction bytes left out where the tool can.
    pub text:       String,
    /// The same, normalised as the duplicate check compares it: no
    /// addresses, labels, comments or salted names.
    pub normalized: String,
}

/// Disassemble the function `symbol` (its name in C, as `#[no_mangle]` or
/// `#[export_name]` gives it) of `library`, with this host's disassembler.
///
/// # Errors
///
/// When this host has no disassembler, when the tool fails on the file, or
/// when the library has no such function.
pub fn disassemble_symbol(library: &Path, symbol: &str) -> Result<SymbolDisassembly, String> {
    let tool = Disassembler::detected().ok_or_else(no_tool)?;
    let text = tool.symbol(library, symbol)?;
    let normalized = normalize_disasm(&text, tool.flavour);
    Ok(SymbolDisassembly {
        tool: tool.clone(),
        text,
        normalized,
    })
}

fn no_tool() -> String {
    "no disassembler found: install llvm-objdump (or the rustup component llvm-tools), \
     binutils objdump, otool, or dumpbin"
        .to_string()
}

/// Whether two variants' disassembly describes the same compiled work.
///
/// `entry_a` / `entry_b` are `bench_entry`'s own disassembly: a cheap negative
/// when both are available. Differing entries prove differing code (the entry
/// is part of it), so the pair is not a duplicate regardless of `text_a` /
/// `text_b`.
///
/// When the entries match, or either is unavailable, the whole disassembly is
/// the authoritative comparison: it sees the user's per-size function wherever
/// the optimizer placed it. `None` on either side never counts as a match:
/// absence of evidence is not evidence of duplication.
fn same_work(
    entry_a: &Option<String>,
    entry_b: &Option<String>,
    text_a: &Option<String>,
    text_b: &Option<String>,
) -> bool {
    if let (Some(a), Some(b)) = (entry_a, entry_b)
        && a != b
    {
        return false;
    }
    matches!((text_a, text_b), (Some(a), Some(b)) if a == b)
}

/// Pairwise duplicate decision over precomputed disassembly, factored out of
/// [`check_duplicates`] so it is testable without a library on disk. Returns
/// the paths of every duplicate pair.
fn duplicate_pairs(
    variant_paths: &[String],
    entry_asm: &[Option<String>],
    text_section: &[Option<String>],
) -> Vec<(String, String)> {
    let mut dupes = Vec::new();
    for i in 0 .. variant_paths.len() {
        for j in (i + 1) .. variant_paths.len() {
            if same_work(
                &entry_asm[i],
                &entry_asm[j],
                &text_section[i],
                &text_section[j],
            ) {
                dupes.push((variant_paths[i].clone(), variant_paths[j].clone()));
            }
        }
    }
    dupes
}

/// The outcome of a duplicate-check pass, kept distinct from printing it so
/// "answered, no duplicates" and "could not answer" are two different values
/// rather than the same empty `dupes` list.
///
/// An empty `dupes` with an empty `unreadable` is a genuine clean pass. A
/// non-empty `unreadable` means the variants named there were excluded from
/// the comparison entirely, because their code could not be disassembled (no
/// disassembler on the host, or it failed on the file).
#[derive(Debug, PartialEq, Eq)]
struct CheckReport {
    /// Duplicate pairs found among variants whose code was read.
    dupes:      Vec<(String, String)>,
    /// Paths whose code could not be read, excluded from `dupes`.
    unreadable: Vec<String>,
}

/// Build a [`CheckReport`] from precomputed disassembly. Pure and testable
/// without a library on disk, same as [`duplicate_pairs`].
fn build_report(
    variant_paths: &[String],
    entry_asm: &[Option<String>],
    text_section: &[Option<String>],
) -> CheckReport {
    let unreadable = variant_paths
        .iter()
        .zip(text_section)
        .filter(|(_, text)| text.is_none())
        .map(|(path, _)| path.clone())
        .collect();
    CheckReport {
        dupes: duplicate_pairs(variant_paths, entry_asm, text_section),
        unreadable,
    }
}

/// The name to print a variant under: its arm name, read off its library's
/// own file name ([`crate::library::arm_name`]).
fn short_name(path: &str) -> &str {
    crate::library::arm_name(path)
}

/// Print a [`CheckReport`] to stderr. Total or partial extraction failure is
/// reported before (and regardless of) any duplicate pairs, so a run that
/// could not fully answer never reads the same as a run that answered "no
/// duplicates".
fn print_report(variant_paths: &[String], report: &CheckReport) {
    if !report.unreadable.is_empty() {
        if report.unreadable.len() == variant_paths.len() {
            eprintln!(
                "  WARNING: could not disassemble any of the {} variant(s) (no disassembler, \
                 or it failed); the duplicate check did not run.",
                variant_paths.len()
            );
        } else {
            eprintln!(
                "  WARNING: {} of {} variant(s) could not be disassembled and were excluded \
                 from the duplicate check:",
                report.unreadable.len(),
                variant_paths.len()
            );
            for path in &report.unreadable {
                eprintln!("    {}", short_name(path));
            }
        }
    }

    if !report.dupes.is_empty() {
        eprintln!(
            "  WARNING: {} variant pair(s) have identical machine code:",
            report.dupes.len()
        );
        for (a, b) in &report.dupes {
            eprintln!("    {} == {}", short_name(a), short_name(b));
        }
    }
}

/// Compare `bench_entry` disassembly and the whole code across all variant
/// libraries with one disassembler, [`Disassembler::detected`]. Reports
/// duplicates to stderr, and reports separately (see [`CheckReport`]) when the
/// check could not disassemble some or all of the variants, so that case is
/// never silently indistinguishable from a clean pass.
pub fn check_duplicates(variant_paths: &[String]) {
    if variant_paths.len() < 2 {
        return;
    }
    let none = vec![None; variant_paths.len()];
    let (entry_asm, text_section) = match Disassembler::detected() {
        Some(tool) => {
            (
                variant_paths
                    .iter()
                    .map(|p| extract_bench_entry(tool, p))
                    .collect(),
                variant_paths
                    .iter()
                    .map(|p| extract_text_section(tool, p))
                    .collect(),
            )
        },
        None => {
            eprintln!("  WARNING: {}", no_tool());
            (none.clone(), none)
        },
    };
    let report = build_report(variant_paths, &entry_asm, &text_section);
    print_report(variant_paths, &report);
}

#[cfg(test)]
#[path = "disasm/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "disasm/tests_live.rs"]
mod tests_live;

#[cfg(test)]
#[path = "disasm/tests_recorded.rs"]
mod tests_recorded;
