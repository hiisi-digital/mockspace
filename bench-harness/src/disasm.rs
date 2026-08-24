//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Automatic disassembly diffing for variant dylibs.
//!
//! Before timing, compare each variant dylib's compiled machine code.
//! If two variants produce identical code, [`check_duplicates`] warns:
//! they will benchmark identically, wasting time.
//!
//! Uses `objdump` (works on Linux + macOS with `llvm-objdump`) with a
//! fallback to `otool -tv` on macOS for older toolchains.
//!
//! The comparison itself has two layers. `bench_entry` (the FFI
//! dispatcher `#[bench_variant]` generates) is cheap to disassemble in
//! isolation and a *different* `bench_entry` proves the variants
//! differ, since the entry is part of the whole `.text` section. It
//! does not prove the converse: the user's per-size function is
//! called through `bench_entry` rather than always inlined into it
//! (a large function, multiple codegen units, or a non-LTO profile
//! can all leave it as a separate symbol), so two variants computing
//! entirely different work can still export a byte-identical
//! dispatcher shell. The authoritative check disassembles the whole
//! `.text` section, which sees the user's function wherever the
//! optimizer put it.
//!
//! When extraction itself fails, silence would look identical to a
//! clean pass: an empty duplicate list either way. [`check_duplicates`]
//! reports that case explicitly (see [`CheckReport`]) rather than
//! letting a variant it could not read vanish into "no duplicates
//! found".

use std::process::Command;

mod normalize;
#[cfg(test)]
use normalize::normalize_addresses;
use normalize::normalize_disasm;

/// The spellings a symbol's C name might carry across platforms.
/// Mach-O prefixes every exported symbol with an underscore
/// (`bench_entry` becomes `_bench_entry`); ELF does not. Try the bare
/// name first (correct on ELF, and objdump accepts it unprefixed
/// there), then the underscore-prefixed form, rather than assuming
/// either spelling.
fn symbol_candidates(symbol: &str) -> [String; 2] {
    [symbol.to_string(), format!("_{symbol}")]
}

/// Disassemble one named symbol from a dylib via `objdump
/// --disassemble-symbols`.
///
/// A candidate that objdump does not find still exits `0`; it only
/// warns on stderr, which this function does not read. So a candidate
/// only counts as found when its own label (`<candidate>:`) actually
/// appears in stdout, not merely when the process exits successfully.
fn objdump_symbol(dylib_path: &str, symbol: &str) -> Option<String> {
    for candidate in symbol_candidates(symbol) {
        let Ok(result) =
            Command::new("objdump")
                .args(["-d", &format!("--disassemble-symbols={candidate}"), dylib_path])
                .output()
        else {
            return None;
        };
        if !result.status.success() {
            continue;
        }
        let text = String::from_utf8_lossy(&result.stdout).to_string();
        if text.contains(&format!("<{candidate}>")) {
            return Some(normalize_disasm(&text));
        }
    }
    None
}

/// Disassemble the whole file via `objdump -d`, no symbol filter.
fn objdump_all(dylib_path: &str) -> Option<String> {
    let result = Command::new("objdump").args(["-d", dylib_path]).output().ok()?;
    if !result.status.success() {
        return None;
    }
    Some(normalize_disasm(&String::from_utf8_lossy(&result.stdout)))
}

/// Disassemble the whole file via `otool -tv` (macOS fallback).
#[cfg(target_os = "macos")]
fn otool_all(dylib_path: &str) -> Option<String> {
    let result = Command::new("otool").args(["-tv", dylib_path]).output().ok()?;
    if !result.status.success() {
        return None;
    }
    Some(normalize_disasm(&String::from_utf8_lossy(&result.stdout)))
}

/// Extract machine code for the `bench_entry` symbol from a dylib.
fn extract_bench_entry(dylib_path: &str) -> Option<String> {
    if let Some(asm) = objdump_symbol(dylib_path, "bench_entry") {
        return Some(asm);
    }

    #[cfg(target_os = "macos")]
    {
        let result = Command::new("otool").args(["-tv", dylib_path]).output().ok()?;

        if result.status.success() {
            let text = String::from_utf8_lossy(&result.stdout).to_string();
            return extract_symbol_range(&text, "_bench_entry");
        }
    }

    None
}

/// Extract lines between a symbol label and the next symbol.
#[cfg(target_os = "macos")]
fn extract_symbol_range(text: &str, symbol: &str) -> Option<String> {
    let mut capture = false;
    let mut lines = Vec::new();
    for line in text.lines() {
        if line.contains(symbol) && line.contains(':') {
            capture = true;
            continue;
        }
        if capture {
            // Stop at next symbol (line ending with ':' that isn't an address)
            if line.contains(':') && !line.starts_with(|c: char| c.is_ascii_hexdigit()) {
                break;
            }
            lines.push(line.to_string());
        }
    }
    if lines.is_empty() { None } else { Some(lines.join("\n")) }
}

/// Disassemble the whole `.text` section of a dylib: every function,
/// not just `bench_entry`. See the module docs for why this, rather
/// than `bench_entry` alone, is the authoritative comparison basis.
///
/// Known gap, not fixed here: `objdump_all` runs `objdump -d`, which
/// disassembles every executable section in the file, while
/// `otool_all` runs `otool -tv`, which covers only `__TEXT,__text`.
/// If `objdump` serves one variant of a pair and the `otool` fallback
/// serves the other (only possible when `objdump` itself is present
/// but fails on exactly one dylib), the two are compared across
/// different byte ranges and can never match. Both binaries reaching
/// for the same tool inside one run is the overwhelmingly common case,
/// so this stays a known gap rather than a fix in this pass.
fn extract_text_section(dylib_path: &str) -> Option<String> {
    if let Some(asm) = objdump_all(dylib_path) {
        return Some(asm);
    }

    #[cfg(target_os = "macos")]
    {
        otool_all(dylib_path)
    }

    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// Whether two variants' disassembly describes the same compiled
/// work.
///
/// Both `entry_asm` and `text_section` are already-extracted, precomputed
/// disassembly; this function does no extraction of its own, so it decides
/// nothing about which subprocess calls happen. `entry_a` / `entry_b` are
/// `bench_entry`'s own disassembly: a cheap negative when both are
/// available. Differing entries prove differing `.text` (the entry is
/// part of it), so the pair is not a duplicate regardless of `text_a` /
/// `text_b`, and the decision does not depend on those having been
/// extractable at all.
///
/// When the entries match, or either is unavailable, the whole `.text`
/// section is the authoritative comparison: it sees the user's
/// per-size function wherever the optimizer placed it, inlined into
/// the dispatcher or standing alone as its own symbol, so two variants
/// computing different work cannot compare equal here even when their
/// dispatcher shells are byte-identical. `None` on either side (the
/// dylib could not be disassembled at all) never counts as a match:
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

/// Pairwise duplicate decision over precomputed disassembly, factored
/// out of [`check_duplicates`] so it is testable without a dylib on
/// disk. Returns the paths of every duplicate pair.
fn duplicate_pairs(
    variant_paths: &[String],
    entry_asm: &[Option<String>],
    text_section: &[Option<String>],
) -> Vec<(String, String)> {
    let mut dupes = Vec::new();
    for i in 0 .. variant_paths.len() {
        for j in (i + 1) .. variant_paths.len() {
            if same_work(&entry_asm[i], &entry_asm[j], &text_section[i], &text_section[j]) {
                dupes.push((variant_paths[i].clone(), variant_paths[j].clone()));
            }
        }
    }
    dupes
}

/// The outcome of a duplicate-check pass, kept distinct from printing
/// it so "answered, no duplicates" and "could not answer" are two
/// different values rather than the same empty `dupes` list.
///
/// An empty `dupes` with an empty `unreadable` is a genuine clean
/// pass. An empty `dupes` with a non-empty `unreadable` means the
/// check could not examine every variant and must not be read as
/// "no duplicates": the variants named in `unreadable` were excluded
/// from the comparison entirely, because their `.text` section could
/// not be extracted (`objdump` and, on macOS, `otool` both failed, or
/// neither is installed). An extraction that silently does nothing
/// must not look like an extraction that ran and found nothing wrong.
#[derive(Debug, PartialEq, Eq)]
struct CheckReport {
    /// Duplicate pairs found among variants whose `.text` section was
    /// extracted successfully.
    dupes:      Vec<(String, String)>,
    /// Paths whose `.text` section could not be extracted at all, and
    /// were therefore excluded from `dupes` regardless of whether they
    /// are, in truth, duplicates of something else in the run.
    unreadable: Vec<String>,
}

/// Build a [`CheckReport`] from precomputed disassembly. Pure and
/// testable without a dylib on disk, same as [`duplicate_pairs`].
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

/// Print a [`CheckReport`] to stderr. Total or partial extraction
/// failure is reported before (and regardless of) any duplicate
/// pairs, so a run that could not fully answer never reads the same
/// as a run that answered "no duplicates".
fn print_report(variant_paths: &[String], report: &CheckReport) {
    if !report.unreadable.is_empty() {
        if report.unreadable.len() == variant_paths.len() {
            eprintln!(
                "  WARNING: could not disassemble any of the {} variant(s) (objdump/otool \
                 unavailable or failed); the duplicate check did not run.",
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
                let short = path.rsplit('/').nth(1).unwrap_or(path);
                eprintln!("    {}", short);
            }
        }
    }

    if !report.dupes.is_empty() {
        eprintln!(
            "  WARNING: {} variant pair(s) have identical machine code:",
            report.dupes.len()
        );
        for (a, b) in &report.dupes {
            let a_short = a.rsplit('/').nth(1).unwrap_or(a);
            let b_short = b.rsplit('/').nth(1).unwrap_or(b);
            eprintln!("    {} == {}", a_short, b_short);
        }
    }
}

/// Compare `bench_entry` disassembly and the whole `.text` section
/// across all variant dylibs. Reports duplicates to stderr, and
/// reports separately (see [`CheckReport`]) when the check could not
/// disassemble some or all of the variants, so that case is never
/// silently indistinguishable from a clean pass.
///
/// Extraction runs eagerly for every variant path before any
/// comparison: the cheap `bench_entry`-only negative in [`same_work`]
/// short-circuits a string comparison once both sides are in memory,
/// not the disassembly itself, which has already happened for every
/// variant by the time this function compares anything.
pub fn check_duplicates(variant_paths: &[String]) {
    if variant_paths.len() < 2 {
        return;
    }

    let entry_asm: Vec<Option<String>> =
        variant_paths.iter().map(|p| extract_bench_entry(p)).collect();
    let text_section: Vec<Option<String>> =
        variant_paths.iter().map(|p| extract_text_section(p)).collect();

    let report = build_report(variant_paths, &entry_asm, &text_section);
    print_report(variant_paths, &report);
}

#[cfg(test)]
#[path = "disasm/tests.rs"]
mod tests;
